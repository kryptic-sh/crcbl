//! The fake `ISteamScreenshots` and `ISteamTimeline`: every call recorded,
//! with what it was given, for the tests to read back.

use std::ffi::{CStr, c_char, c_void};

use super::{accessor, script};
use crate::ffi::{
    ISteamScreenshots, ISteamTimeline, SteamApiCall,
    manifest::{ScreenshotsFns, TimelineFns},
};

/// What the fake screenshot library answers, and what it has seen.
#[derive(Debug, Default)]
pub(crate) struct FakeScreenshots {
    pub(crate) hooked: bool,
    pub(crate) triggered: u32,
    /// Every `(pixels, size, width, height)` `WriteScreenshot` was given.
    pub(crate) written: Vec<(Vec<u8>, u32, i32, i32)>,
    /// What `WriteScreenshot` answers.
    pub(crate) handle: u32,
    /// Every `(screenshot, location)` and `(screenshot, user)`.
    pub(crate) locations: Vec<(u32, String)>,
    pub(crate) tags: Vec<(u32, u64)>,
}

/// What the fake timeline has seen: each call as one line — its name, then
/// its arguments, `|`-separated — in order.
#[derive(Debug, Default)]
pub(crate) struct FakeTimeline {
    pub(crate) log: Vec<String>,
    /// What the event calls answer.
    pub(crate) event: u64,
    /// What the recording calls answer; `0` is `k_uAPICallInvalid`.
    pub(crate) call: SteamApiCall,
}

fn text(ptr: *const c_char) -> String {
    // SAFETY: every caller passes a NUL-terminated string.
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

fn log(line: String) {
    script(|s| s.timeline.log.push(line));
}

pub(super) const SCREENSHOTS: ScreenshotsFns = ScreenshotsFns {
    accessor: screenshots_accessor,
    write_screenshot: fake_write_screenshot,
    trigger_screenshot: fake_trigger_screenshot,
    hook_screenshots: fake_hook_screenshots,
    is_screenshots_hooked: fake_is_screenshots_hooked,
    set_location: fake_set_location,
    tag_user: fake_tag_user,
};

unsafe extern "C" fn screenshots_accessor() -> *mut c_void {
    accessor(crate::ffi::versions::SCREENSHOTS.accessor)
}

unsafe extern "C" fn fake_write_screenshot(
    _: *mut ISteamScreenshots,
    rgb: *mut c_void,
    size: u32,
    width: i32,
    height: i32,
) -> u32 {
    let len = usize::try_from(size).unwrap();
    // SAFETY: the caller passes `size` readable bytes.
    let pixels = unsafe { core::slice::from_raw_parts(rgb.cast::<u8>(), len) }.to_vec();
    script(|s| {
        s.screenshots.written.push((pixels, size, width, height));
        s.screenshots.handle
    })
}

unsafe extern "C" fn fake_trigger_screenshot(_: *mut ISteamScreenshots) {
    script(|s| s.screenshots.triggered += 1);
}

unsafe extern "C" fn fake_hook_screenshots(_: *mut ISteamScreenshots, hook: bool) {
    script(|s| s.screenshots.hooked = hook);
}

unsafe extern "C" fn fake_is_screenshots_hooked(_: *mut ISteamScreenshots) -> bool {
    script(|s| s.screenshots.hooked)
}

unsafe extern "C" fn fake_set_location(
    _: *mut ISteamScreenshots,
    screenshot: u32,
    location: *const c_char,
) -> bool {
    let location = text(location);
    script(|s| {
        s.screenshots.locations.push((screenshot, location));
        !s.refuse
    })
}

unsafe extern "C" fn fake_tag_user(_: *mut ISteamScreenshots, screenshot: u32, user: u64) -> bool {
    script(|s| {
        s.screenshots.tags.push((screenshot, user));
        !s.refuse
    })
}

pub(super) const TIMELINE: TimelineFns = TimelineFns {
    accessor: timeline_accessor,
    set_timeline_tooltip: fake_tooltip,
    clear_timeline_tooltip: fake_clear_tooltip,
    set_timeline_game_mode: fake_game_mode,
    add_instantaneous_timeline_event: fake_instant,
    add_range_timeline_event: fake_range,
    start_range_timeline_event: fake_start,
    update_range_timeline_event: fake_update,
    end_range_timeline_event: fake_end,
    remove_timeline_event: fake_remove,
    does_event_recording_exist: fake_event_recording,
    start_game_phase: fake_start_phase,
    end_game_phase: fake_end_phase,
    set_game_phase_id: fake_phase_id,
    does_game_phase_recording_exist: fake_phase_recording,
    add_game_phase_tag: fake_phase_tag,
    set_game_phase_attribute: fake_phase_attribute,
    open_overlay_to_game_phase: fake_overlay_phase,
    open_overlay_to_timeline_event: fake_overlay_event,
};

unsafe extern "C" fn fake_tooltip(_: *mut ISteamTimeline, description: *const c_char, delta: f32) {
    log(format!("tooltip|{}|{delta}", text(description)));
}

unsafe extern "C" fn fake_clear_tooltip(_: *mut ISteamTimeline, delta: f32) {
    log(format!("clear tooltip|{delta}"));
}

unsafe extern "C" fn fake_game_mode(_: *mut ISteamTimeline, mode: i32) {
    log(format!("game mode|{mode}"));
}

unsafe extern "C" fn fake_end(_: *mut ISteamTimeline, event: u64, offset: f32) {
    log(format!("end|{event}|{offset}"));
}

unsafe extern "C" fn fake_remove(_: *mut ISteamTimeline, event: u64) {
    log(format!("remove|{event}"));
}

unsafe extern "C" fn fake_event_recording(_: *mut ISteamTimeline, event: u64) -> SteamApiCall {
    log(format!("event recording|{event}"));
    script(|s| s.timeline.call)
}

unsafe extern "C" fn fake_start_phase(_: *mut ISteamTimeline) {
    log("start phase".to_owned());
}

unsafe extern "C" fn fake_end_phase(_: *mut ISteamTimeline) {
    log("end phase".to_owned());
}

unsafe extern "C" fn fake_phase_id(_: *mut ISteamTimeline, id: *const c_char) {
    log(format!("phase id|{}", text(id)));
}

unsafe extern "C" fn fake_phase_recording(
    _: *mut ISteamTimeline,
    id: *const c_char,
) -> SteamApiCall {
    log(format!("phase recording|{}", text(id)));
    script(|s| s.timeline.call)
}

unsafe extern "C" fn fake_phase_tag(
    _: *mut ISteamTimeline,
    name: *const c_char,
    icon: *const c_char,
    group: *const c_char,
    priority: u32,
) {
    log(format!(
        "phase tag|{}|{}|{}|{priority}",
        text(name),
        text(icon),
        text(group)
    ));
}

unsafe extern "C" fn fake_phase_attribute(
    _: *mut ISteamTimeline,
    group: *const c_char,
    value: *const c_char,
    priority: u32,
) {
    log(format!(
        "phase attribute|{}|{}|{priority}",
        text(group),
        text(value)
    ));
}

unsafe extern "C" fn fake_overlay_phase(_: *mut ISteamTimeline, id: *const c_char) {
    log(format!("overlay phase|{}", text(id)));
}

unsafe extern "C" fn fake_overlay_event(_: *mut ISteamTimeline, event: u64) {
    log(format!("overlay event|{event}"));
}

unsafe extern "C" fn timeline_accessor() -> *mut c_void {
    accessor(crate::ffi::versions::TIMELINE.accessor)
}

/// The three strings every event call takes, joined.
fn strings(title: *const c_char, description: *const c_char, icon: *const c_char) -> String {
    format!("{}|{}|{}", text(title), text(description), text(icon))
}

unsafe extern "C" fn fake_instant(
    _: *mut ISteamTimeline,
    title: *const c_char,
    description: *const c_char,
    icon: *const c_char,
    priority: u32,
    offset: f32,
    clip: i32,
) -> u64 {
    let strings = strings(title, description, icon);
    log(format!("instant|{strings}|{priority}|{offset}|{clip}"));
    script(|s| s.timeline.event)
}

unsafe extern "C" fn fake_range(
    _: *mut ISteamTimeline,
    title: *const c_char,
    description: *const c_char,
    icon: *const c_char,
    priority: u32,
    offset: f32,
    duration: f32,
    clip: i32,
) -> u64 {
    let strings = strings(title, description, icon);
    log(format!(
        "range|{strings}|{priority}|{offset}|{duration}|{clip}"
    ));
    script(|s| s.timeline.event)
}

unsafe extern "C" fn fake_start(
    _: *mut ISteamTimeline,
    title: *const c_char,
    description: *const c_char,
    icon: *const c_char,
    priority: u32,
    offset: f32,
    clip: i32,
) -> u64 {
    let strings = strings(title, description, icon);
    log(format!("start|{strings}|{priority}|{offset}|{clip}"));
    script(|s| s.timeline.event)
}

unsafe extern "C" fn fake_update(
    _: *mut ISteamTimeline,
    event: u64,
    title: *const c_char,
    description: *const c_char,
    icon: *const c_char,
    priority: u32,
    clip: i32,
) {
    let strings = strings(title, description, icon);
    log(format!("update|{event}|{strings}|{priority}|{clip}"));
}
