//! A screen reader's side of AT-SPI, for the Wayland and X11 suites: switch
//! the accessibility bus on, find a window and its button by name the way Orca
//! walks the registry, and invoke the button. Through `busctl`, which every
//! systemd system has, rather than a D-Bus crate every test build would compile.
//!
//! Switching AT-SPI on (`org.a11y.Status.IsEnabled`) changes every program on
//! that session bus, so [`private_bus`] refuses to go on unless the harness
//! started one with `dbus-run-session` and said so.

use accesskit::{Action, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
use crcbl_shell::{ShellEvent, WindowId};

/// The tree's root, the window itself.
const ROOT: NodeId = NodeId(1);
/// The one button under it.
const BUTTON: NodeId = NodeId(2);
/// What the root is called, and what the reader finds the window by.
pub const WINDOW_LABEL: &str = "crcbl accessibility";
/// What the button is called, and what the reader finds it by.
const BUTTON_LABEL: &str = "Press me";

/// A window labelled [`WINDOW_LABEL`] holding one clickable button.
pub fn tree() -> TreeUpdate {
    let mut root = Node::new(Role::Window);
    root.set_label(WINDOW_LABEL);
    root.set_children(vec![BUTTON]);
    let mut button = Node::new(Role::Button);
    button.set_label(BUTTON_LABEL);
    button.add_action(Action::Click);
    TreeUpdate {
        nodes: vec![(ROOT, root), (BUTTON, button)],
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

/// The reader: finds the button by name under the window by name, under any
/// application the registry lists, and invokes its first action. An `Err` is
/// "not there yet", and the caller asks again.
pub fn press_the_button(bus: &str) -> Result<(), String> {
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
                if label(bus, &name, &node).as_deref() != Some(BUTTON_LABEL) {
                    continue;
                }
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
                return if reply.trim() == "b true" {
                    Ok(())
                } else {
                    Err(format!("DoAction answered {reply:?}"))
                };
            }
        }
    }
    Err("no window labelled with the button under it yet".to_string())
}
