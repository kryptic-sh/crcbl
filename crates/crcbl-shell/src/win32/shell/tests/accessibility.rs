//! A window's tree read through UI Automation by a real client, the way a
//! screen reader reads it.
//!
//! The client runs on a second thread of this process, with its own COM
//! apartment, while the test thread keeps pumping the shell: every question the
//! client asks reaches the window as a message, and only a pumped window
//! answers. Nothing here sends the window procedure a message by hand — the
//! `WM_GETOBJECT`s and the invoke come from UI Automation itself.

use core::ffi::c_void;
use std::thread;
use std::time::{Duration, Instant};

use accesskit::{Action, ActionRequest, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern,
    TreeScope_Descendants, UIA_InvokePatternId, UIA_NamePropertyId,
};

use super::desktop::shell;
use super::{hwnd_of, window};
use crate::{Shell, ShellCaps, ShellEvent};

/// The tree's root, the window itself.
const ROOT: NodeId = NodeId(1);
/// The one button under it.
const BUTTON: NodeId = NodeId(2);
/// What the root is called.
const WINDOW_LABEL: &str = "crcbl accessibility";
/// What the button is called, and what the client looks it up by.
const BUTTON_LABEL: &str = "Press me";
/// The ceiling on the whole exchange, from the client's first question to the
/// press arriving as an event. Generous, because a cold UI Automation start on
/// a shared runner is slow; a pass takes a fraction of it.
const PATIENCE: Duration = Duration::from_secs(20);
/// How long the test thread waits for messages between pumps, and the client
/// between lookups.
const TICK: Duration = Duration::from_millis(10);

/// A window labelled [`WINDOW_LABEL`] holding one clickable button.
fn tree() -> TreeUpdate {
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

/// The client: joins the multithreaded apartment, finds the button under
/// `hwnd` by name and invokes it.
fn press_the_button(hwnd: isize, deadline: Instant) -> Result<(), String> {
    // SAFETY: initialising COM on this thread, which the client owns for its
    // whole life; balanced below.
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
        .ok()
        .map_err(|error| format!("CoInitializeEx refused the client thread: {error}"))?;
    let pressed = invoke_by_name(hwnd, deadline);
    // SAFETY: balances the initialisation above, which succeeded; every COM
    // object this thread made lived inside `invoke_by_name` and is released.
    unsafe { CoUninitialize() };
    pressed
}

/// [`press_the_button`] inside its apartment.
fn invoke_by_name(hwnd: isize, deadline: Instant) -> Result<(), String> {
    // SAFETY: creating the system's UI Automation client object in-process,
    // on a thread with COM initialised.
    let automation: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
            .map_err(|error| format!("no CUIAutomation: {error}"))?;
    let name = VARIANT::from(BUTTON_LABEL);
    // SAFETY: a property condition over a live `VARIANT` the call copies.
    let condition = unsafe { automation.CreatePropertyCondition(UIA_NamePropertyId, &name) }
        .map_err(|error| format!("no name condition: {error}"))?;
    let button: IUIAutomationElement = loop {
        // Asked afresh each time: the first answer is the adapter's
        // placeholder, because the tree is only sent once the program has
        // heard the activation this very lookup caused.
        //
        // SAFETY: `hwnd` is the test's window, which the test thread keeps
        // alive until this thread has been joined. The lookup sends it
        // `WM_GETOBJECT`, which that thread's pump answers.
        let found =
            unsafe { automation.ElementFromHandle(HWND(hwnd as *mut c_void)) }.and_then(|root| {
                // SAFETY: a search under a live element with a live condition.
                unsafe { root.FindFirst(TreeScope_Descendants, &condition) }
            });
        match found {
            Ok(button) => break button,
            Err(error) if Instant::now() >= deadline => {
                return Err(format!(
                    "no element named {BUTTON_LABEL:?} under the window within {PATIENCE:?}; \
                     the last lookup said {error}"
                ));
            }
            Err(_) => thread::sleep(TICK),
        }
    };
    // SAFETY: asking a live element for a pattern it may not support, which
    // is an error rather than undefined behaviour.
    let invoke: IUIAutomationInvokePattern =
        unsafe { button.GetCurrentPatternAs(UIA_InvokePatternId) }
            .map_err(|error| format!("the button has no Invoke pattern: {error}"))?;
    // SAFETY: invoking through a live pattern object.
    unsafe { invoke.Invoke() }.map_err(|error| format!("Invoke failed: {error}"))
}

#[test]
#[ignore = "opens windows on the real desktop: run-win32-e2e.ps1 runs it"]
fn a_reader_finds_the_button_in_the_tree_and_presses_it() {
    let mut shell = shell();
    assert!(
        shell.caps().contains(ShellCaps::ACCESSIBILITY),
        "the accesskit build claims the cap: {:?}",
        shell.caps()
    );
    let window = window(&mut shell);
    shell.pump(&mut |_| {});
    let hwnd = hwnd_of(&shell, window) as isize;

    let deadline = Instant::now() + PATIENCE;
    let mut client = Some(thread::spawn(move || press_the_button(hwnd, deadline)));
    let mut client_result = None;
    let mut seen = Vec::new();
    let mut activations = 0;
    let mut press: Option<ActionRequest> = None;
    // Pumped until the client has finished as well as until the press has
    // arrived: a client left mid-lookup is blocked on a message only this
    // thread can answer, and joining it unpumped would never return.
    loop {
        shell.wait_events(Some(TICK));
        let mut events = Vec::new();
        shell.pump(&mut |event| events.push(event));
        for event in events {
            seen.push(event.name());
            match event {
                ShellEvent::AccessibilityActivated { window: read } if read == window => {
                    activations += 1;
                    shell
                        .update_accessibility(window, tree())
                        .expect("the window is live");
                }
                ShellEvent::AccessibilityAction {
                    window: target,
                    request,
                } if target == window => press = Some(request),
                _ => {}
            }
        }
        if let Some(done) = client.take_if(|handle| handle.is_finished()) {
            let result = done.join().expect("the client thread does not panic");
            if let Err(why) = &result {
                panic!("the client gave up: {why}; the shell raised {seen:?}");
            }
            client_result = Some(result);
        }
        let now = Instant::now();
        if client.is_none() && (press.is_some() || now >= deadline) {
            break;
        }
        assert!(
            now < deadline + PATIENCE,
            "the client is still running {PATIENCE:?} past its own deadline; the shell raised \
             {seen:?}"
        );
    }

    assert_eq!(
        client_result,
        Some(Ok(())),
        "the client finds and invokes the button; the shell raised {seen:?}"
    );
    assert_eq!(
        activations, 1,
        "one activation however many times the client asks; the shell raised {seen:?}"
    );
    let press = press.unwrap_or_else(|| {
        panic!("no AccessibilityAction within {PATIENCE:?}; the shell raised {seen:?}")
    });
    assert_eq!(
        (press.action, press.target_tree, press.target_node),
        (Action::Click, TreeId::ROOT, BUTTON),
        "the invoke is a click on the button: {press:?}"
    );
    shell.destroy_window(window).expect("destroy");
}
