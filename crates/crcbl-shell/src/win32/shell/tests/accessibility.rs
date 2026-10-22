//! A window's tree read through UI Automation by a real client, the way a
//! screen reader reads it.
//!
//! The client runs on a second thread of this process, with its own COM
//! apartment, while the test thread keeps pumping the shell: every question the
//! client asks reaches the window as a message, and only a pumped window
//! answers. Nothing here sends the window procedure a message by hand — the
//! `WM_GETOBJECT`s and the invoke come from UI Automation itself.

use core::ffi::c_void;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use accesskit::{
    Action, ActionRequest, Node, NodeId, Role, TextPosition, TextSelection, TreeId, TreeInfo,
    TreeUpdate,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern,
    IUIAutomationTextPattern, IUIAutomationValuePattern, TextPatternRangeEndpoint_End,
    TextPatternRangeEndpoint_Start, TreeScope_Descendants, UIA_InvokePatternId, UIA_NamePropertyId,
    UIA_TextPatternId, UIA_ValuePatternId,
};

use super::Win32Shell;
use super::desktop::{DesktopShell, shell};
use super::{hwnd_of, window};
use crate::{Shell, ShellCaps, ShellEvent, WindowId};

/// The tree's root, the window itself.
const ROOT: NodeId = NodeId(1);
/// The one button under it.
const BUTTON: NodeId = NodeId(2);
/// A text field beside the button, and the run holding its text.
const FIELD: NodeId = NodeId(3);
const FIELD_RUN: NodeId = NodeId(4);
/// What the root is called.
const WINDOW_LABEL: &str = "crcbl accessibility";
/// What the button is called, and what the client looks it up by.
const BUTTON_LABEL: &str = "Press me";
/// What the field is called, and what the client looks it up by.
const FIELD_LABEL: &str = "Query";
/// What the field holds: a two-byte character, so a caret counted in bytes and
/// one counted in characters disagree.
const FIELD_TEXT: &str = "héllo";
/// The field's text before its caret, which sits after "hé".
const BEFORE_CARET: &str = "hé";
/// The ceiling on the whole exchange, from the client's first question to the
/// press arriving as an event. Generous, because a cold UI Automation start on
/// a shared runner is slow; a pass takes a fraction of it.
const PATIENCE: Duration = Duration::from_secs(20);
/// How long the test thread waits for messages between pumps, and the client
/// between lookups.
const TICK: Duration = Duration::from_millis(10);

/// A window labelled [`WINDOW_LABEL`] holding one clickable button and one
/// text field, shaped as `crcbl-ui` builds a text field: a `TextInput` with one
/// `TextRun` child and the caret as a selection on that run, as
/// `accesskit_windows` builds its text pattern from.
fn tree() -> TreeUpdate {
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
        character_index: BEFORE_CARET.chars().count(),
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

/// Runs `ask` on a client thread that joins the multithreaded apartment for
/// as long as it asks, as a screen reader's does.
fn client<T: Send + 'static>(
    hwnd: isize,
    deadline: Instant,
    ask: fn(&IUIAutomation, isize, Instant) -> Result<T, String>,
) -> JoinHandle<Result<T, String>> {
    thread::spawn(move || {
        // SAFETY: initialising COM on this thread, which the client owns for
        // its whole life; balanced below.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(|error| format!("CoInitializeEx refused the client thread: {error}"))?;
        // SAFETY: creating the system's UI Automation client object
        // in-process, on a thread with COM initialised.
        let answer = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
            .map_err(|error| format!("no CUIAutomation: {error}"))
            .and_then(|automation: IUIAutomation| ask(&automation, hwnd, deadline));
        // SAFETY: balances the initialisation above, which succeeded; every
        // COM object this thread made was released when `ask` returned.
        unsafe { CoUninitialize() };
        answer
    })
}

/// The element named `name` under `hwnd`, asked for until `deadline`.
fn find_by_name(
    automation: &IUIAutomation,
    hwnd: isize,
    name: &str,
    deadline: Instant,
) -> Result<IUIAutomationElement, String> {
    let wanted = VARIANT::from(name);
    // SAFETY: a property condition over a live `VARIANT` the call copies.
    let condition = unsafe { automation.CreatePropertyCondition(UIA_NamePropertyId, &wanted) }
        .map_err(|error| format!("no name condition: {error}"))?;
    loop {
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
            Ok(element) => return Ok(element),
            Err(error) if Instant::now() >= deadline => {
                return Err(format!(
                    "no element named {name:?} under the window within {PATIENCE:?}; the last \
                     lookup said {error}"
                ));
            }
            Err(_) => thread::sleep(TICK),
        }
    }
}

/// Finds the button by name and invokes it.
fn press_the_button(
    automation: &IUIAutomation,
    hwnd: isize,
    deadline: Instant,
) -> Result<(), String> {
    let button = find_by_name(automation, hwnd, BUTTON_LABEL, deadline)?;
    // SAFETY: asking a live element for a pattern it may not support, which
    // is an error rather than undefined behaviour.
    let invoke: IUIAutomationInvokePattern =
        unsafe { button.GetCurrentPatternAs(UIA_InvokePatternId) }
            .map_err(|error| format!("the button has no Invoke pattern: {error}"))?;
    // SAFETY: invoking through a live pattern object.
    unsafe { invoke.Invoke() }.map_err(|error| format!("Invoke failed: {error}"))
}

/// What a reader hears of the field: its value through the Value pattern, and
/// the text before its caret through the Text pattern, the document range cut
/// at the selection's start.
fn read_the_field(
    automation: &IUIAutomation,
    hwnd: isize,
    deadline: Instant,
) -> Result<(String, String), String> {
    let field = find_by_name(automation, hwnd, FIELD_LABEL, deadline)?;
    // SAFETY: asking a live element for patterns it may not support, which is
    // an error rather than undefined behaviour; each read is of a live object.
    unsafe {
        let value: IUIAutomationValuePattern = field
            .GetCurrentPatternAs(UIA_ValuePatternId)
            .map_err(|error| format!("the field has no Value pattern: {error}"))?;
        let value = value
            .CurrentValue()
            .map_err(|error| format!("the field's value did not read: {error}"))?
            .to_string();
        let text: IUIAutomationTextPattern = field
            .GetCurrentPatternAs(UIA_TextPatternId)
            .map_err(|error| format!("the field has no Text pattern: {error}"))?;
        let selection = text
            .GetSelection()
            .map_err(|error| format!("the field's selection did not read: {error}"))?;
        let caret = selection
            .GetElement(0)
            .map_err(|error| format!("the field reports no selection: {error}"))?;
        let before = text
            .DocumentRange()
            .map_err(|error| format!("the field has no document range: {error}"))?;
        before
            .MoveEndpointByRange(
                TextPatternRangeEndpoint_End,
                &caret,
                TextPatternRangeEndpoint_Start,
            )
            .map_err(|error| format!("the range did not cut at the caret: {error}"))?;
        let before = before
            .GetText(-1)
            .map_err(|error| format!("the text before the caret did not read: {error}"))?
            .to_string();
        Ok((value, before))
    }
}

/// What the shell raised while a client asked.
struct Exchange<T> {
    /// What the client answered.
    answer: Result<T, String>,
    /// Every event's name, for the failure messages.
    seen: Vec<&'static str>,
    activations: usize,
    press: Option<ActionRequest>,
}

/// Pumps `shell` for `window` until `asking` has finished and, when
/// `until_press`, until an action has arrived too — answering each activation
/// with [`tree`]. A client left mid-lookup is blocked on a message only this
/// thread can answer, so joining it unpumped would never return.
fn exchange<T>(
    shell: &mut Win32Shell,
    window: WindowId,
    asking: JoinHandle<Result<T, String>>,
    deadline: Instant,
    until_press: bool,
) -> Exchange<T> {
    let mut asking = Some(asking);
    let mut answer = None;
    let mut seen = Vec::new();
    let mut activations = 0;
    let mut press = None;
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
        if let Some(done) = asking.take_if(|handle| handle.is_finished()) {
            answer = Some(done.join().expect("the client thread does not panic"));
        }
        let now = Instant::now();
        if asking.is_none() && (!until_press || press.is_some() || now >= deadline) {
            break;
        }
        assert!(
            now < deadline + PATIENCE,
            "the client is still running {PATIENCE:?} past its own deadline; the shell raised \
             {seen:?}"
        );
    }
    Exchange {
        answer: answer.expect("the loop ends only once the client has answered"),
        seen,
        activations,
        press,
    }
}

/// A shell claiming accessibility, one window on it, pumped once.
fn open() -> (DesktopShell, WindowId, isize) {
    let mut shell = shell();
    assert!(
        shell.caps().contains(ShellCaps::ACCESSIBILITY),
        "the accesskit build claims the cap: {:?}",
        shell.caps()
    );
    let window = window(&mut shell);
    shell.pump(&mut |_| {});
    let hwnd = hwnd_of(&shell, window) as isize;
    (shell, window, hwnd)
}

#[test]
#[ignore = "opens windows on the real desktop: run-win32-e2e.ps1 runs it"]
fn a_reader_finds_the_button_in_the_tree_and_presses_it() {
    let (mut shell, window, hwnd) = open();
    let deadline = Instant::now() + PATIENCE;
    let asking = client(hwnd, deadline, press_the_button);
    let done = exchange(&mut shell, window, asking, deadline, true);
    let seen = &done.seen;
    if let Err(why) = &done.answer {
        panic!("the client gave up: {why}; the shell raised {seen:?}");
    }
    assert_eq!(
        done.activations, 1,
        "one activation however many times the client asks; the shell raised {seen:?}"
    );
    let press = done.press.unwrap_or_else(|| {
        panic!("no AccessibilityAction within {PATIENCE:?}; the shell raised {seen:?}")
    });
    assert_eq!(
        (press.action, press.target_tree, press.target_node),
        (Action::Click, TreeId::ROOT, BUTTON),
        "the invoke is a click on the button: {press:?}"
    );
    shell.destroy_window(window).expect("destroy");
}

/// **A text field reads as one: its value, and its caret counted in
/// characters.** The shape `crcbl-ui` gives a drawn field through
/// `Ui::set_accessible_text_input` and its own `Ui::text_input`, read as a
/// reader on Windows reads it, through the Value and Text patterns
/// `accesskit_windows` builds from it.
#[test]
#[ignore = "opens windows on the real desktop: run-win32-e2e.ps1 runs it"]
fn a_reader_reads_the_fields_value_and_caret() {
    let (mut shell, window, hwnd) = open();
    let deadline = Instant::now() + PATIENCE;
    let asking = client(hwnd, deadline, read_the_field);
    let done = exchange(&mut shell, window, asking, deadline, false);
    let seen = &done.seen;
    let (value, before) = done
        .answer
        .unwrap_or_else(|why| panic!("the client gave up: {why}; the shell raised {seen:?}"));
    assert_eq!(value, FIELD_TEXT, "the field's value");
    assert_eq!(
        before, BEFORE_CARET,
        "the caret sits after {BEFORE_CARET:?}, two characters and three bytes in"
    );
    shell.destroy_window(window).expect("destroy");
}
