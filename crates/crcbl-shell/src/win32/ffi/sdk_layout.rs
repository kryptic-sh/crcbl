//! Every structure in [`ffi`](super) against the `windows` crate's
//! declaration of it: size, alignment, and each field's offset and type,
//! including the union arms a declaration here flattens into one field. See
//! [`sdk_witness`](crate::win32::sdk_witness) for the rows.
//!
//! The parent's `the_structures_match_the_c_layout` checks offsets and widths
//! against a C probe's numbers and runs on every host; this runs on Windows
//! only, and is the one that sees a field of the right width and the wrong
//! signedness.

use super::*;
use crate::win32::sdk_witness::{Witnessed, witness};
use windows::Win32::Devices::Display::{
    DISPLAYCONFIG_2DREGION, DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_MODE_INFO,
    DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_PATH_SOURCE_INFO, DISPLAYCONFIG_PATH_TARGET_INFO,
    DISPLAYCONFIG_RATIONAL, DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_MODE,
    DISPLAYCONFIG_VIDEO_SIGNAL_INFO,
};
use windows::Win32::Foundation::{LUID, POINT, POINTL, RECT};
use windows::Win32::Graphics::Gdi::{DEVMODEW, MONITORINFOEXW};
use windows::Win32::UI::Input::Ime::{CANDIDATEFORM, COMPOSITIONFORM};
use windows::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT;
use windows::Win32::UI::Input::{
    RAWINPUT, RAWINPUT_0, RAWINPUTDEVICE, RAWINPUTHEADER, RAWKEYBOARD, RAWMOUSE,
};
use windows::Win32::UI::Shell::{APPBARDATA, DROPFILES};
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, MINMAXINFO, MSG, WINDOWPLACEMENT, WNDCLASSEXW,
};

witness!(point_matches_the_sdk: struct Point => POINT {
    x: is x;
    y: is y;
});
// `DEVMODEW::dmPosition` is a `POINTL`, the same two `LONG`s under another
// name.
witness!(point_matches_the_sdk_pointl: struct Point => POINTL {
    x: is x;
    y: is y;
});
witness!(rect_matches_the_sdk: struct Rect => RECT {
    left: is left;
    top: is top;
    right: is right;
    bottom: is bottom;
});
witness!(app_bar_data_matches_the_sdk: struct AppBarData => APPBARDATA {
    cb_size: is cbSize;
    hwnd: is hWnd.0;
    callback_message: is uCallbackMessage;
    edge: is uEdge;
    rc: nested rc;
    l_param: is lParam.0;
});
witness!(msg_matches_the_sdk: struct Msg => MSG {
    hwnd: is hwnd.0;
    message: is message;
    w_param: is wParam.0;
    l_param: is lParam.0;
    time: is time;
    pt: nested pt;
    // Declared only under `_MAC`, so absent from the SDK's metadata: it sits
    // in the tail padding after `pt`. See the type docs.
    l_private: after pt;
});
witness!(wnd_class_ex_w_matches_the_sdk: struct WndClassExW => WNDCLASSEXW {
    cb_size: is cbSize;
    style: is style.0;
    lpfn_wnd_proc: width lpfnWndProc;
    cb_cls_extra: is cbClsExtra;
    cb_wnd_extra: is cbWndExtra;
    h_instance: is hInstance.0;
    h_icon: is hIcon.0;
    h_cursor: is hCursor.0;
    hbr_background: is hbrBackground.0;
    lpsz_menu_name: is lpszMenuName.0;
    lpsz_class_name: is lpszClassName.0;
    h_icon_sm: is hIconSm.0;
});
witness!(create_struct_w_matches_the_sdk: struct CreateStructW => CREATESTRUCTW {
    lp_create_params: is lpCreateParams;
    h_instance: is hInstance.0;
    h_menu: is hMenu.0;
    hwnd_parent: is hwndParent.0;
    cy: is cy;
    cx: is cx;
    y: is y;
    x: is x;
    style: is style;
    lpsz_name: is lpszName.0;
    lpsz_class: is lpszClass.0;
    dw_ex_style: is dwExStyle.0;
});
witness!(min_max_info_matches_the_sdk: struct MinMaxInfo => MINMAXINFO {
    pt_reserved: nested ptReserved;
    pt_max_size: nested ptMaxSize;
    pt_max_position: nested ptMaxPosition;
    pt_min_track_size: nested ptMinTrackSize;
    pt_max_track_size: nested ptMaxTrackSize;
});
witness!(window_placement_matches_the_sdk: struct WindowPlacement => WINDOWPLACEMENT {
    length: is length;
    flags: is flags.0;
    show_cmd: is showCmd;
    pt_min_position: nested ptMinPosition;
    pt_max_position: nested ptMaxPosition;
    rc_normal_position: nested rcNormalPosition;
});
// The SDK nests `MONITORINFO` as the first member; the declaration here
// spells its fields out.
witness!(monitor_info_ex_w_matches_the_sdk: struct MonitorInfoExW => MONITORINFOEXW {
    cb_size: is monitorInfo.cbSize;
    rc_monitor: nested monitorInfo.rcMonitor;
    rc_work: nested monitorInfo.rcWork;
    dw_flags: is monitorInfo.dwFlags;
    sz_device: is szDevice;
});
witness!(dev_mode_w_matches_the_sdk: struct DevModeW => DEVMODEW {
    dm_device_name: is dmDeviceName;
    dm_spec_version: is dmSpecVersion;
    dm_driver_version: is dmDriverVersion;
    dm_size: is dmSize;
    dm_driver_extra: is dmDriverExtra;
    dm_fields: is dmFields.0;
    // The printer/display union, by its display arm.
    dm_position: nested Anonymous1.Anonymous2.dmPosition;
    dm_display_orientation: is Anonymous1.Anonymous2.dmDisplayOrientation.0;
    dm_display_fixed_output: is Anonymous1.Anonymous2.dmDisplayFixedOutput.0;
    dm_color: is dmColor.0;
    dm_duplex: is dmDuplex.0;
    dm_y_resolution: is dmYResolution;
    dm_tt_option: is dmTTOption.0;
    dm_collate: is dmCollate.0;
    dm_form_name: is dmFormName;
    dm_log_pixels: is dmLogPixels;
    dm_bits_per_pel: is dmBitsPerPel;
    dm_pels_width: is dmPelsWidth;
    dm_pels_height: is dmPelsHeight;
    dm_display_flags: is Anonymous2.dmDisplayFlags;
    dm_display_frequency: is dmDisplayFrequency;
    dm_icm_method: is dmICMMethod;
    dm_icm_intent: is dmICMIntent;
    dm_media_type: is dmMediaType;
    dm_dither_type: is dmDitherType;
    dm_reserved1: is dmReserved1;
    dm_reserved2: is dmReserved2;
    dm_panning_width: is dmPanningWidth;
    dm_panning_height: is dmPanningHeight;
} also {
    dm_display_flags: is Anonymous2.dmNup;
});

witness!(luid_matches_the_sdk: struct Luid => LUID {
    low_part: is LowPart;
    high_part: is HighPart;
});
witness!(display_config_2d_region_matches_the_sdk: struct DisplayConfig2dRegion => DISPLAYCONFIG_2DREGION {
    cx: is cx;
    cy: is cy;
});
witness!(display_config_rational_matches_the_sdk: struct DisplayConfigRational => DISPLAYCONFIG_RATIONAL {
    numerator: is Numerator;
    denominator: is Denominator;
});
witness!(display_config_video_signal_info_matches_the_sdk: struct DisplayConfigVideoSignalInfo => DISPLAYCONFIG_VIDEO_SIGNAL_INFO {
    pixel_rate: is pixelRate;
    h_sync_freq: nested hSyncFreq;
    v_sync_freq: nested vSyncFreq;
    active_size: nested activeSize;
    total_size: nested totalSize;
    // The bitfield arm, which `monitors` reads the divider out of.
    additional_signal_info: is Anonymous.AdditionalSignalInfo._bitfield;
    scan_line_ordering: is scanLineOrdering.0;
} also {
    additional_signal_info: is Anonymous.videoStandard;
});
witness!(display_config_target_mode_matches_the_sdk: struct DisplayConfigTargetMode => DISPLAYCONFIG_TARGET_MODE {
    target_video_signal_info: nested targetVideoSignalInfo;
});
witness!(display_config_mode_info_matches_the_sdk: struct DisplayConfigModeInfo => DISPLAYCONFIG_MODE_INFO {
    info_type: is infoType.0;
    id: is id;
    adapter_id: nested adapterId;
    // The union's target arm, the only one read.
    target_mode: nested Anonymous.targetMode;
});
witness!(display_config_path_source_info_matches_the_sdk: struct DisplayConfigPathSourceInfo => DISPLAYCONFIG_PATH_SOURCE_INFO {
    adapter_id: nested adapterId;
    id: is id;
    mode_info_idx: is Anonymous.modeInfoIdx;
    status_flags: is statusFlags;
} also {
    mode_info_idx: is Anonymous.Anonymous._bitfield;
});
witness!(display_config_path_target_info_matches_the_sdk: struct DisplayConfigPathTargetInfo => DISPLAYCONFIG_PATH_TARGET_INFO {
    adapter_id: nested adapterId;
    id: is id;
    mode_info_idx: is Anonymous.modeInfoIdx;
    output_technology: is outputTechnology.0;
    rotation: is rotation.0;
    scaling: is scaling.0;
    refresh_rate: nested refreshRate;
    scan_line_ordering: is scanLineOrdering.0;
    target_available: is targetAvailable.0;
    status_flags: is statusFlags;
} also {
    mode_info_idx: is Anonymous.Anonymous._bitfield;
});
witness!(display_config_path_info_matches_the_sdk: struct DisplayConfigPathInfo => DISPLAYCONFIG_PATH_INFO {
    source_info: nested sourceInfo;
    target_info: nested targetInfo;
    flags: is flags;
});
witness!(display_config_device_info_header_matches_the_sdk: struct DisplayConfigDeviceInfoHeader => DISPLAYCONFIG_DEVICE_INFO_HEADER {
    kind: is r#type.0;
    size: is size;
    adapter_id: nested adapterId;
    id: is id;
});
witness!(display_config_source_device_name_matches_the_sdk: struct DisplayConfigSourceDeviceName => DISPLAYCONFIG_SOURCE_DEVICE_NAME {
    header: nested header;
    view_gdi_device_name: is viewGdiDeviceName;
});

witness!(track_mouse_matches_the_sdk: struct TrackMouse => TRACKMOUSEEVENT {
    cb_size: is cbSize;
    dw_flags: is dwFlags.0;
    hwnd_track: is hwndTrack.0;
    dw_hover_time: is dwHoverTime;
});
// 4-aligned here, byte-packed in the SDK; see the type docs for why that is
// safe in the one direction it is used.
witness!(drop_files_matches_the_sdk: packed DropFiles => DROPFILES {
    p_files: is pFiles;
    pt: nested pt;
    f_nc: is fNC.0;
    f_wide: is fWide.0;
});
witness!(raw_input_device_matches_the_sdk: struct RawInputDevice => RAWINPUTDEVICE {
    us_usage_page: is usUsagePage;
    us_usage: is usUsage;
    dw_flags: is dwFlags.0;
    hwnd_target: is hwndTarget.0;
});
witness!(raw_input_header_matches_the_sdk: struct RawInputHeader => RAWINPUTHEADER {
    dw_type: is dwType;
    dw_size: is dwSize;
    h_device: is hDevice.0;
    w_param: is wParam.0;
});
witness!(raw_mouse_matches_the_sdk: struct RawMouse => RAWMOUSE {
    us_flags: is usFlags.0;
    // The union's alignment, which C leaves unnamed.
    padding: after usFlags;
    ul_buttons: is Anonymous.ulButtons;
    ul_raw_buttons: is ulRawButtons;
    l_last_x: is lLastX;
    l_last_y: is lLastY;
    ul_extra_information: is ulExtraInformation;
});
witness!(raw_keyboard_matches_the_sdk: struct RawKeyboard => RAWKEYBOARD {
    make_code: is MakeCode;
    flags: is Flags;
    reserved: is Reserved;
    v_key: is VKey;
    message: is Message;
    extra_information: is ExtraInformation;
});
// `RAWHID`, the arm left out, is variable-length and smaller than both; the
// size check is what says leaving it out changed nothing.
witness!(raw_input_data_matches_the_sdk: union RawInputData => RAWINPUT_0 {
    mouse: nested mouse;
    keyboard: nested keyboard;
});
witness!(raw_input_matches_the_sdk: struct RawInput => RAWINPUT {
    header: nested header;
    data: nested data;
});

witness!(composition_form_matches_the_sdk: struct CompositionForm => COMPOSITIONFORM {
    dw_style: is dwStyle;
    pt_current_pos: nested ptCurrentPos;
    rc_area: nested rcArea;
});
witness!(candidate_form_matches_the_sdk: struct CandidateForm => CANDIDATEFORM {
    dw_index: is dwIndex;
    dw_style: is dwStyle;
    pt_current_pos: nested ptCurrentPos;
    rc_area: nested rcArea;
});

/// `RawMouse::ul_buttons` is the button union's `ULONG` arm, and its docs say
/// the other arm's two `USHORT`s are its halves — `proc` reads `usButtonFlags`
/// as the low one. On a little-endian target the low half is the lower
/// address, so each must start where its half does.
#[test]
fn the_raw_mouse_button_halves_are_the_sdks() {
    let buttons = core::mem::offset_of!(RawMouse, ul_buttons);
    assert_eq!(
        core::mem::offset_of!(RAWMOUSE, Anonymous.Anonymous.usButtonFlags),
        buttons,
        "usButtonFlags is the low half"
    );
    assert_eq!(
        core::mem::offset_of!(RAWMOUSE, Anonymous.Anonymous.usButtonData),
        buttons + size_of::<u16>(),
        "usButtonData is the high half"
    );
}
