//! RandR requests on a [`Connection`], and the owned values their replies are
//! copied into.
//!
//! The shapes follow the protocol (`randrproto.txt`) rather than any one
//! tool's model of a display, because the consumer this was built for
//! transcribes `xrandr`'s algorithm and needs every field `xrandr` reads —
//! including both timestamps a `SetCrtcConfig` is checked against. Each method
//! names the RandR version its request arrived in, and returns
//! [`Error::RandrTooOld`] on a server older than that.

use core::ops::BitOr;

use crate::connection::{Atom, Connection, Error, RandrVersion, Reply, Timestamp, Window};
use crate::ffi::{self, RandrModeInfo, RenderTransform};

/// A RandR CRTC id.
pub type Crtc = u32;
/// A RandR output id.
pub type Output = u32;
/// A RandR mode id.
pub type Mode = u32;

/// `xcb_randr_rotation_t`: a rotation and reflection, as bits.
///
/// A CRTC's `rotation` has exactly one of the `ROTATE_*` bits and any of the
/// `REFLECT_*` ones; its `rotations` has every bit the CRTC can do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rotation(pub u16);

impl Rotation {
    /// Not rotated.
    pub const ROTATE_0: Self = Self(1);
    /// Rotated 90° counter-clockwise.
    pub const ROTATE_90: Self = Self(1 << 1);
    /// Rotated 180°.
    pub const ROTATE_180: Self = Self(1 << 2);
    /// Rotated 270° counter-clockwise.
    pub const ROTATE_270: Self = Self(1 << 3);
    /// Mirrored left to right.
    pub const REFLECT_X: Self = Self(1 << 4);
    /// Mirrored top to bottom.
    pub const REFLECT_Y: Self = Self(1 << 5);

    /// Whether every bit of `other` is set in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for Rotation {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// `xcb_randr_notify_mask_t`: which RandR events
/// [`Connection::select_input`] asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NotifyMask(pub u16);

impl NotifyMask {
    /// No events — what deselects them all.
    pub const NONE: Self = Self(0);
    /// `ScreenChangeNotify`: the screen's size or rotation changed.
    pub const SCREEN_CHANGE: Self = Self(1);
    /// `Notify` with `CrtcChange`.
    pub const CRTC_CHANGE: Self = Self(1 << 1);
    /// `Notify` with `OutputChange` — including a monitor plugged or pulled.
    pub const OUTPUT_CHANGE: Self = Self(1 << 2);
    /// `Notify` with `OutputProperty`.
    pub const OUTPUT_PROPERTY: Self = Self(1 << 3);
    /// `Notify` with `ProviderChange`.
    pub const PROVIDER_CHANGE: Self = Self(1 << 4);
    /// `Notify` with `ProviderProperty`.
    pub const PROVIDER_PROPERTY: Self = Self(1 << 5);
    /// `Notify` with `ResourceChange`.
    pub const RESOURCE_CHANGE: Self = Self(1 << 6);
    /// `Notify` with `Lease`.
    pub const LEASE: Self = Self(1 << 7);
}

impl BitOr for NotifyMask {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// `xcb_randr_mode_flag_t` bits, for [`ModeInfo::flags`].
pub mod mode_flag {
    /// Positive horizontal sync.
    pub const HSYNC_POSITIVE: u32 = 1;
    /// Negative horizontal sync.
    pub const HSYNC_NEGATIVE: u32 = 1 << 1;
    /// Positive vertical sync.
    pub const VSYNC_POSITIVE: u32 = 1 << 2;
    /// Negative vertical sync.
    pub const VSYNC_NEGATIVE: u32 = 1 << 3;
    /// Interlaced.
    pub const INTERLACE: u32 = 1 << 4;
    /// Each line scanned twice.
    pub const DOUBLE_SCAN: u32 = 1 << 5;
    /// Composite sync.
    pub const CSYNC: u32 = 1 << 6;
    /// Positive composite sync.
    pub const CSYNC_POSITIVE: u32 = 1 << 7;
    /// Negative composite sync.
    pub const CSYNC_NEGATIVE: u32 = 1 << 8;
    /// `hskew` is meaningful.
    pub const HSKEW_PRESENT: u32 = 1 << 9;
    /// Broadcast.
    pub const BCAST: u32 = 1 << 10;
    /// Pixel multiplexed.
    pub const PIXEL_MULTIPLEX: u32 = 1 << 11;
    /// Each frame scanned twice.
    pub const DOUBLE_CLOCK: u32 = 1 << 12;
    /// Half the clock.
    pub const HALVE_CLOCK: u32 = 1 << 13;
}

/// One video mode: its timings and its name.
///
/// The refresh rate is not a field: it is `dot_clock / (htotal * vtotal)`,
/// adjusted for [`mode_flag::INTERLACE`] and [`mode_flag::DOUBLE_SCAN`] as
/// `xrandr` does.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ModeInfo {
    /// The mode's id; ignored by [`Connection::create_mode`], which is given
    /// a new one.
    pub id: Mode,
    /// Visible width.
    pub width: u16,
    /// Visible height.
    pub height: u16,
    /// Pixel clock in Hz.
    pub dot_clock: u32,
    /// Horizontal sync start.
    pub hsync_start: u16,
    /// Horizontal sync end.
    pub hsync_end: u16,
    /// Horizontal total.
    pub htotal: u16,
    /// Horizontal skew.
    pub hskew: u16,
    /// Vertical sync start.
    pub vsync_start: u16,
    /// Vertical sync end.
    pub vsync_end: u16,
    /// Vertical total.
    pub vtotal: u16,
    /// [`mode_flag`] bits.
    pub flags: u32,
    /// The name, `1920x1080` say. Bytes that are not UTF-8 become U+FFFD.
    pub name: String,
}

impl ModeInfo {
    /// The wire form, with the name's length.
    fn to_raw(&self) -> Result<RandrModeInfo, Error> {
        Ok(RandrModeInfo {
            id: self.id,
            width: self.width,
            height: self.height,
            dot_clock: self.dot_clock,
            hsync_start: self.hsync_start,
            hsync_end: self.hsync_end,
            htotal: self.htotal,
            hskew: self.hskew,
            vsync_start: self.vsync_start,
            vsync_end: self.vsync_end,
            vtotal: self.vtotal,
            name_len: u16::try_from(self.name.len())
                .map_err(|_| Error::TooLong { what: "mode name" })?,
            mode_flags: self.flags,
        })
    }
}

/// `GetScreenResourcesCurrent`'s reply.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ScreenResources {
    /// When the configuration was last set: what a `SetCrtcConfig`'s
    /// `timestamp` is compared to.
    pub timestamp: Timestamp,
    /// When the server last changed its configuration — a monitor plugged in,
    /// say: what a `SetCrtcConfig`'s `config_timestamp` must equal.
    pub config_timestamp: Timestamp,
    /// Every CRTC.
    pub crtcs: Vec<Crtc>,
    /// Every output.
    pub outputs: Vec<Output>,
    /// Every mode any output has, its name split out of the reply's name
    /// block.
    pub modes: Vec<ModeInfo>,
}

/// `GetScreenSizeRange`'s reply, in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ScreenSizeRange {
    /// Smallest width.
    pub min_width: u16,
    /// Smallest height.
    pub min_height: u16,
    /// Largest width.
    pub max_width: u16,
    /// Largest height.
    pub max_height: u16,
}

/// Whether an output has a monitor attached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OutputConnection {
    /// A monitor is attached.
    Connected,
    /// Nothing is attached.
    Disconnected,
    /// The hardware cannot tell.
    Unknown,
    /// A value this crate does not know.
    Other(u8),
}

impl OutputConnection {
    pub(crate) const fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Connected,
            1 => Self::Disconnected,
            2 => Self::Unknown,
            other => Self::Other(other),
        }
    }
}

/// `xcb_render_sub_pixel_t`: the order of an output's subpixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SubPixel {
    /// Not known.
    Unknown,
    /// Red, green, blue, left to right.
    HorizontalRgb,
    /// Blue, green, red, left to right.
    HorizontalBgr,
    /// Red, green, blue, top to bottom.
    VerticalRgb,
    /// Blue, green, red, top to bottom.
    VerticalBgr,
    /// No subpixels — a projector, say.
    None,
    /// A value this crate does not know.
    Other(u16),
}

impl SubPixel {
    pub(crate) const fn from_raw(raw: u16) -> Self {
        match raw {
            0 => Self::Unknown,
            1 => Self::HorizontalRgb,
            2 => Self::HorizontalBgr,
            3 => Self::VerticalRgb,
            4 => Self::VerticalBgr,
            5 => Self::None,
            other => Self::Other(other),
        }
    }
}

/// `GetOutputInfo`'s reply.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OutputInfo {
    /// When the configuration was last set.
    pub timestamp: Timestamp,
    /// The CRTC driving the output, or `0`.
    pub crtc: Crtc,
    /// Physical width in millimetres.
    pub mm_width: u32,
    /// Physical height in millimetres.
    pub mm_height: u32,
    /// Whether a monitor is attached.
    pub connection: OutputConnection,
    /// Subpixel order.
    pub subpixel: SubPixel,
    /// The CRTCs that can drive it.
    pub crtcs: Vec<Crtc>,
    /// The modes it supports; the first `num_preferred` are preferred.
    pub modes: Vec<Mode>,
    /// How many of `modes`, from the front, are preferred.
    pub num_preferred: u16,
    /// The outputs it can be cloned with.
    pub clones: Vec<Output>,
    /// The connector's name, `HDMI-1` say. Bytes that are not UTF-8 become
    /// U+FFFD.
    pub name: String,
}

/// `GetCrtcInfo`'s reply.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct CrtcInfo {
    /// When the configuration was last set.
    pub timestamp: Timestamp,
    /// Left edge in the screen.
    pub x: i16,
    /// Top edge.
    pub y: i16,
    /// Width in pixels, after rotation.
    pub width: u16,
    /// Height in pixels, after rotation.
    pub height: u16,
    /// The mode it drives, or `0` when it is disabled.
    pub mode: Mode,
    /// Its rotation and reflection.
    pub rotation: Rotation,
    /// Every rotation and reflection it can do.
    pub rotations: Rotation,
    /// The outputs it drives.
    pub outputs: Vec<Output>,
    /// The outputs it could drive.
    pub possible: Vec<Output>,
}

/// One monitor from `GetMonitors`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct MonitorInfo {
    /// Its name, as an atom; [`Connection::atom_name`] reads it.
    pub name: Atom,
    /// Whether it is the primary monitor.
    pub primary: bool,
    /// Whether the server made it from a CRTC, rather than a client defining
    /// it.
    pub automatic: bool,
    /// Left edge in the screen.
    pub x: i16,
    /// Top edge.
    pub y: i16,
    /// Width in pixels.
    pub width: u16,
    /// Height in pixels.
    pub height: u16,
    /// Physical width in millimetres.
    pub mm_width: u32,
    /// Physical height in millimetres.
    pub mm_height: u32,
    /// The outputs it covers.
    pub outputs: Vec<Output>,
}

/// `GetMonitors`'s reply.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Monitors {
    /// When the configuration was last set.
    pub timestamp: Timestamp,
    /// The monitors.
    pub monitors: Vec<MonitorInfo>,
}

/// `xcb_randr_set_config_t`: what the server did with a `SetCrtcConfig` or
/// `SetPanning`.
///
/// Only [`Success`](Self::Success) changed anything. The two time statuses
/// are the server refusing a change computed from a stale view: re-read the
/// resources and try again. The protocol defines them; the X.Org server's
/// `SetCrtcConfig` checks neither timestamp and never answers them, so a
/// caller cannot rely on them to catch a race there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SetConfigStatus {
    /// Applied.
    Success,
    /// `config_timestamp` is not the server's: its configuration changed
    /// (a monitor came or went) since the caller read it.
    InvalidConfigTime,
    /// `timestamp` is older than the last configuration set.
    InvalidTime,
    /// The driver refused the configuration.
    Failed,
    /// A value this crate does not know.
    Other(u8),
}

impl SetConfigStatus {
    pub(crate) const fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Success,
            1 => Self::InvalidConfigTime,
            2 => Self::InvalidTime,
            3 => Self::Failed,
            other => Self::Other(other),
        }
    }
}

/// The reply to a `SetCrtcConfig` or `SetPanning`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SetConfigReply {
    /// What the server did.
    pub status: SetConfigStatus,
    /// The configuration's timestamp afterwards.
    pub timestamp: Timestamp,
}

/// What [`Connection::set_crtc_config`] sets a CRTC to.
///
/// `mode` `0` with no outputs disables the CRTC, and that is the default —
/// with [`Rotation::ROTATE_0`], because the server refuses a rotation with no
/// rotate bit (`BadValue`), whatever else the request asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CrtcConfig<'a> {
    /// Left edge in the screen.
    pub x: i16,
    /// Top edge.
    pub y: i16,
    /// The mode to drive, or `0`.
    pub mode: Mode,
    /// Rotation and reflection.
    pub rotation: Rotation,
    /// The outputs to drive.
    pub outputs: &'a [Output],
}

impl Default for CrtcConfig<'_> {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            mode: 0,
            rotation: Rotation::ROTATE_0,
            outputs: &[],
        }
    }
}

/// A CRTC's panning, as `GetPanning` reports it and `SetPanning` takes it.
///
/// All zero is panning off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Panning {
    /// When it was last set; on [`Connection::set_panning`], the request's
    /// timestamp, `0` for now.
    pub timestamp: Timestamp,
    /// Left edge of the panning area.
    pub left: u16,
    /// Top edge.
    pub top: u16,
    /// Width.
    pub width: u16,
    /// Height.
    pub height: u16,
    /// Left edge of the area the pointer is tracked in.
    pub track_left: u16,
    /// Top edge.
    pub track_top: u16,
    /// Width.
    pub track_width: u16,
    /// Height.
    pub track_height: u16,
    /// Distance from the left edge at which panning starts.
    pub border_left: i16,
    /// From the top edge.
    pub border_top: i16,
    /// From the right edge.
    pub border_right: i16,
    /// From the bottom edge.
    pub border_bottom: i16,
}

/// A projective transform: a 3×3 matrix of 16.16 fixed-point numbers, row by
/// row, as RENDER defines it. `65536` is `1.0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Transform {
    /// The matrix, `matrix[row][column]`.
    pub matrix: [[i32; 3]; 3],
}

impl Transform {
    /// `1.0` in 16.16 fixed point.
    pub const ONE: i32 = 1 << 16;
    /// No transformation.
    pub const IDENTITY: Self = Self {
        matrix: [[Self::ONE, 0, 0], [0, Self::ONE, 0], [0, 0, Self::ONE]],
    };

    const fn from_raw(raw: &RenderTransform) -> Self {
        let m = raw.matrix;
        Self {
            matrix: [[m[0], m[1], m[2]], [m[3], m[4], m[5]], [m[6], m[7], m[8]]],
        }
    }

    const fn to_raw(self) -> RenderTransform {
        let [a, b, c] = self.matrix;
        RenderTransform {
            matrix: [a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2]],
        }
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// `GetCrtcTransform`'s reply.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CrtcTransform {
    /// Whether the CRTC can be transformed at all.
    pub has_transforms: bool,
    /// The transform the next `SetCrtcConfig` applies.
    pub pending: Transform,
    /// The filter it applies with — `bilinear`, `nearest` — or empty.
    pub pending_filter: String,
    /// The filter's parameters, 16.16 fixed point.
    pub pending_params: Vec<i32>,
    /// The transform in effect.
    pub current: Transform,
    /// The filter in effect.
    pub current_filter: String,
    /// Its parameters.
    pub current_params: Vec<i32>,
}

/// An output property's value, its format being which arm it is.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PropertyValue {
    /// Format 8: bytes.
    Format8(Vec<u8>),
    /// Format 16.
    Format16(Vec<u16>),
    /// Format 32: atoms, integers, XIDs.
    Format32(Vec<u32>),
}

impl PropertyValue {
    /// Bits per item: `8`, `16` or `32`.
    #[must_use]
    pub const fn format(&self) -> u8 {
        match self {
            Self::Format8(_) => 8,
            Self::Format16(_) => 16,
            Self::Format32(_) => 32,
        }
    }

    /// How many items it holds.
    #[must_use]
    pub const fn len(&self) -> usize {
        match self {
            Self::Format8(items) => items.len(),
            Self::Format16(items) => items.len(),
            Self::Format32(items) => items.len(),
        }
    }

    /// Whether it holds none.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// `GetOutputProperty`'s reply.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OutputPropertyData {
    /// The property's type, or `0` when it does not exist.
    pub type_: Atom,
    /// Bytes left after what was returned. When the requested type did not
    /// match, the protocol says the whole value's size in bytes; the X.Org
    /// server sends its length in items instead.
    pub bytes_after: u32,
    /// The items returned, or `None` when the property does not exist.
    pub value: Option<PropertyValue>,
}

/// `QueryOutputProperty`'s reply.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct PropertyInfo {
    /// Whether a change waits for the next `SetCrtcConfig`.
    pub pending: bool,
    /// Whether `valid_values` is a `[min, max]` range rather than a list.
    pub range: bool,
    /// Whether clients may change it.
    pub immutable: bool,
    /// The values it accepts; empty when it accepts any.
    pub valid_values: Vec<i32>,
}

/// How [`Connection::change_output_property`] combines the new items with the
/// old.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PropertyMode {
    /// Replace the value.
    Replace,
    /// Put the items before the old ones; the format must match.
    Prepend,
    /// Put the items after the old ones; the format must match.
    Append,
}

impl Connection {
    /// `QueryVersion`, asking for [`RandrVersion::REQUESTED`]; returns what the
    /// server settled on. [`Connection::connect`] has already asked, and
    /// [`Connection::randr`] holds its answer; this asks again.
    ///
    /// # Errors
    ///
    /// Why RandR cannot be used, or what the request met.
    pub fn query_version(&self) -> Result<RandrVersion, Error> {
        let randr = self.randr_at(1, 0)?;
        self.query_version_with(randr)
    }

    /// `GetScreenResourcesCurrent` (RandR 1.3): the CRTCs, outputs and modes,
    /// as the server last saw them, without probing the hardware.
    ///
    /// # Errors
    ///
    /// Why RandR cannot be used, or what the request met.
    pub fn screen_resources_current(&self, window: Window) -> Result<ScreenResources, Error> {
        let randr = self.randr_at(1, 3)?;
        // SAFETY: the connection is live.
        let cookie = unsafe { (randr.get_screen_resources_current)(self.raw(), window) };
        let reply = self.reply("GetScreenResourcesCurrent", |error| {
            // SAFETY: the cookie is this connection's, for this request.
            unsafe { (randr.get_screen_resources_current_reply)(self.raw(), cookie, error) }
        })?;
        // SAFETY: each accessor reads only the live reply's header to locate
        // its list; `list` bounds-checks what it then copies.
        let lists = unsafe {
            (
                (randr.resources_crtcs)(reply.as_ptr()),
                (randr.resources_outputs)(reply.as_ptr()),
                (randr.resources_modes)(reply.as_ptr()),
                (randr.resources_names)(reply.as_ptr()),
            )
        };
        screen_resources_from(&reply, lists)
    }

    /// `GetScreenResources` (RandR 1.2): as
    /// [`screen_resources_current`](Self::screen_resources_current), after the
    /// server probes the hardware — which is how a monitor plugged in since is
    /// found, and what `xrandr` asks for by default. The probe can take the
    /// server a noticeable time, during which it serves no one.
    ///
    /// # Errors
    ///
    /// Why RandR cannot be used, or what the request met.
    pub fn screen_resources(&self, window: Window) -> Result<ScreenResources, Error> {
        let randr = self.randr_at(1, 2)?;
        // SAFETY: the connection is live.
        let cookie = unsafe { (randr.get_screen_resources)(self.raw(), window) };
        let reply = self.reply("GetScreenResources", |error| {
            // SAFETY: the cookie is this connection's, for this request.
            unsafe { (randr.get_screen_resources_reply)(self.raw(), cookie, error) }
        })?;
        // SAFETY: as in `screen_resources_current`, with this request's own
        // accessors.
        let lists = unsafe {
            (
                (randr.probed_resources_crtcs)(reply.as_ptr()),
                (randr.probed_resources_outputs)(reply.as_ptr()),
                (randr.probed_resources_modes)(reply.as_ptr()),
                (randr.probed_resources_names)(reply.as_ptr()),
            )
        };
        screen_resources_from(&reply, lists)
    }

    /// `GetScreenSizeRange` (RandR 1.2): the sizes
    /// [`set_screen_size`](Self::set_screen_size) accepts.
    ///
    /// # Errors
    ///
    /// Why RandR cannot be used, or what the request met.
    pub fn screen_size_range(&self, window: Window) -> Result<ScreenSizeRange, Error> {
        let randr = self.randr_at(1, 2)?;
        // SAFETY: the connection is live.
        let cookie = unsafe { (randr.get_screen_size_range)(self.raw(), window) };
        let reply = self.reply("GetScreenSizeRange", |error| {
            // SAFETY: the cookie is this connection's, for this request.
            unsafe { (randr.get_screen_size_range_reply)(self.raw(), cookie, error) }
        })?;
        Ok(ScreenSizeRange {
            min_width: reply.min_width,
            min_height: reply.min_height,
            max_width: reply.max_width,
            max_height: reply.max_height,
        })
    }

    /// `GetOutputInfo` (RandR 1.2), as of `config_timestamp` — the one
    /// [`ScreenResources`] carries.
    ///
    /// # Errors
    ///
    /// `BadRROutput` for an output that does not exist, why RandR cannot be
    /// used, or what else the request met.
    pub fn output_info(
        &self,
        output: Output,
        config_timestamp: Timestamp,
    ) -> Result<OutputInfo, Error> {
        let randr = self.randr_at(1, 2)?;
        // SAFETY: the connection is live.
        let cookie = unsafe { (randr.get_output_info)(self.raw(), output, config_timestamp) };
        let reply = self.reply("GetOutputInfo", |error| {
            // SAFETY: the cookie is this connection's, for this request.
            unsafe { (randr.get_output_info_reply)(self.raw(), cookie, error) }
        })?;
        // SAFETY: each accessor reads only the live reply's header to locate
        // its list; `list` bounds-checks what it then copies.
        let (crtcs, modes, clones, name) = unsafe {
            (
                (randr.output_crtcs)(reply.as_ptr()),
                (randr.output_modes)(reply.as_ptr()),
                (randr.output_clones)(reply.as_ptr()),
                (randr.output_name)(reply.as_ptr()),
            )
        };
        let name = reply.list(name, usize::from(reply.name_len))?;
        Ok(OutputInfo {
            timestamp: reply.timestamp,
            crtc: reply.crtc,
            mm_width: reply.mm_width,
            mm_height: reply.mm_height,
            connection: OutputConnection::from_raw(reply.connection),
            subpixel: SubPixel::from_raw(u16::from(reply.subpixel_order)),
            crtcs: reply.list(crtcs, usize::from(reply.num_crtcs))?,
            modes: reply.list(modes, usize::from(reply.num_modes))?,
            num_preferred: reply.num_preferred,
            clones: reply.list(clones, usize::from(reply.num_clones))?,
            name: String::from_utf8_lossy(&name).into_owned(),
        })
    }

    /// `GetCrtcInfo` (RandR 1.2), as of `config_timestamp`.
    ///
    /// # Errors
    ///
    /// `BadRRCrtc` for a CRTC that does not exist, why RandR cannot be used,
    /// or what else the request met.
    pub fn crtc_info(&self, crtc: Crtc, config_timestamp: Timestamp) -> Result<CrtcInfo, Error> {
        let randr = self.randr_at(1, 2)?;
        // SAFETY: the connection is live.
        let cookie = unsafe { (randr.get_crtc_info)(self.raw(), crtc, config_timestamp) };
        let reply = self.reply("GetCrtcInfo", |error| {
            // SAFETY: the cookie is this connection's, for this request.
            unsafe { (randr.get_crtc_info_reply)(self.raw(), cookie, error) }
        })?;
        // SAFETY: each accessor reads only the live reply's header to locate
        // its list; `list` bounds-checks what it then copies.
        let (outputs, possible) = unsafe {
            (
                (randr.crtc_outputs)(reply.as_ptr()),
                (randr.crtc_possible)(reply.as_ptr()),
            )
        };
        Ok(CrtcInfo {
            timestamp: reply.timestamp,
            x: reply.x,
            y: reply.y,
            width: reply.width,
            height: reply.height,
            mode: reply.mode,
            rotation: Rotation(reply.rotation),
            rotations: Rotation(reply.rotations),
            outputs: reply.list(outputs, usize::from(reply.num_outputs))?,
            possible: reply.list(possible, usize::from(reply.num_possible_outputs))?,
        })
    }

    /// `GetOutputPrimary` (RandR 1.3): the primary output, or `0`.
    ///
    /// # Errors
    ///
    /// Why RandR cannot be used, or what the request met.
    pub fn output_primary(&self, window: Window) -> Result<Output, Error> {
        let randr = self.randr_at(1, 3)?;
        // SAFETY: the connection is live.
        let cookie = unsafe { (randr.get_output_primary)(self.raw(), window) };
        let reply = self.reply("GetOutputPrimary", |error| {
            // SAFETY: the cookie is this connection's, for this request.
            unsafe { (randr.get_output_primary_reply)(self.raw(), cookie, error) }
        })?;
        Ok(reply.output)
    }

    /// `SetOutputPrimary` (RandR 1.3); `0` clears it.
    ///
    /// # Errors
    ///
    /// Why RandR cannot be used, or what the request met.
    pub fn set_output_primary(&self, window: Window, output: Output) -> Result<(), Error> {
        let randr = self.randr_at(1, 3)?;
        // SAFETY: the connection is live.
        self.check(unsafe { (randr.set_output_primary_checked)(self.raw(), window, output) })
    }

    /// `GetMonitors` (RandR 1.5). With `get_active`, only monitors that are
    /// lit — what a desktop lays itself out on.
    ///
    /// # Errors
    ///
    /// Why RandR cannot be used, or what the request met.
    pub fn monitors(&self, window: Window, get_active: bool) -> Result<Monitors, Error> {
        let randr = self.randr_at(1, 5)?;
        // SAFETY: the connection is live.
        let cookie = unsafe { (randr.get_monitors)(self.raw(), window, u8::from(get_active)) };
        let reply = self.reply("GetMonitors", |error| {
            // SAFETY: the cookie is this connection's, for this request.
            unsafe { (randr.get_monitors_reply)(self.raw(), cookie, error) }
        })?;
        // SAFETY: the reply is live; the iterator only locates the first
        // monitor and counts `n_monitors`.
        let mut iterator = unsafe { (randr.monitors_iterator)(reply.as_ptr()) };
        let mut monitors = Vec::new();
        while iterator.rem > 0 {
            // `list` bounds-checks the fixed part before it is read, and the
            // outputs after it, so a count that runs off the reply stops here.
            let [info] = reply
                .list(iterator.data.cast_const(), 1)?
                .try_into()
                .map_err(|_| Error::MalformedReply {
                    request: "GetMonitors",
                })?;
            // SAFETY: `iterator.data` was just shown to be a whole monitor
            // inside the reply.
            let outputs = unsafe { (randr.monitor_info_outputs)(iterator.data) };
            monitors.push(MonitorInfo {
                name: info.name,
                primary: info.primary != 0,
                automatic: info.automatic != 0,
                x: info.x,
                y: info.y,
                width: info.width,
                height: info.height,
                mm_width: info.width_in_millimeters,
                mm_height: info.height_in_millimeters,
                outputs: reply.list(outputs, usize::from(info.n_output))?,
            });
            // SAFETY: the iterator is on a monitor whose outputs were just
            // shown to be inside the reply, so stepping past them is in
            // bounds, and the next turn checks the next monitor before
            // reading it.
            unsafe { (randr.monitor_info_next)(&mut iterator) };
        }
        Ok(Monitors {
            timestamp: reply.timestamp,
            monitors,
        })
    }

    /// `GetCrtcGammaSize` (RandR 1.2): entries in each of the CRTC's gamma
    /// ramps.
    ///
    /// # Errors
    ///
    /// `BadRRCrtc` for a CRTC that does not exist, why RandR cannot be used,
    /// or what else the request met.
    pub fn crtc_gamma_size(&self, crtc: Crtc) -> Result<u16, Error> {
        let randr = self.randr_at(1, 2)?;
        // SAFETY: the connection is live.
        let cookie = unsafe { (randr.get_crtc_gamma_size)(self.raw(), crtc) };
        let reply = self.reply("GetCrtcGammaSize", |error| {
            // SAFETY: the cookie is this connection's, for this request.
            unsafe { (randr.get_crtc_gamma_size_reply)(self.raw(), cookie, error) }
        })?;
        Ok(reply.size)
    }

    /// `GetPanning` (RandR 1.3).
    ///
    /// # Errors
    ///
    /// `BadRRCrtc` for a CRTC that does not exist, why RandR cannot be used,
    /// or what else the request met.
    pub fn get_panning(&self, crtc: Crtc) -> Result<Panning, Error> {
        let randr = self.randr_at(1, 3)?;
        // SAFETY: the connection is live.
        let cookie = unsafe { (randr.get_panning)(self.raw(), crtc) };
        let reply = self.reply("GetPanning", |error| {
            // SAFETY: the cookie is this connection's, for this request.
            unsafe { (randr.get_panning_reply)(self.raw(), cookie, error) }
        })?;
        Ok(Panning {
            timestamp: reply.timestamp,
            left: reply.left,
            top: reply.top,
            width: reply.width,
            height: reply.height,
            track_left: reply.track_left,
            track_top: reply.track_top,
            track_width: reply.track_width,
            track_height: reply.track_height,
            border_left: reply.border_left,
            border_top: reply.border_top,
            border_right: reply.border_right,
            border_bottom: reply.border_bottom,
        })
    }

    /// `SetPanning` (RandR 1.3). `panning.timestamp` is the request's.
    ///
    /// # Errors
    ///
    /// `BadRRCrtc` for a CRTC that does not exist — and from a server whose
    /// driver cannot pan, which is how Xvfb answers — why RandR cannot be
    /// used, or what else the request met.
    pub fn set_panning(&self, crtc: Crtc, panning: &Panning) -> Result<SetConfigReply, Error> {
        let randr = self.randr_at(1, 3)?;
        let p = panning;
        // SAFETY: the connection is live; the arguments are plain integers.
        let cookie = unsafe {
            (randr.set_panning)(
                self.raw(),
                crtc,
                p.timestamp,
                p.left,
                p.top,
                p.width,
                p.height,
                p.track_left,
                p.track_top,
                p.track_width,
                p.track_height,
                p.border_left,
                p.border_top,
                p.border_right,
                p.border_bottom,
            )
        };
        let reply = self.reply("SetPanning", |error| {
            // SAFETY: the cookie is this connection's, for this request.
            unsafe { (randr.set_panning_reply)(self.raw(), cookie, error) }
        })?;
        Ok(SetConfigReply {
            status: SetConfigStatus::from_raw(reply.status),
            timestamp: reply.timestamp,
        })
    }

    /// `GetCrtcTransform` (RandR 1.3).
    ///
    /// # Errors
    ///
    /// `BadRRCrtc` for a CRTC that does not exist, why RandR cannot be used,
    /// or what else the request met.
    pub fn crtc_transform(&self, crtc: Crtc) -> Result<CrtcTransform, Error> {
        let randr = self.randr_at(1, 3)?;
        // SAFETY: the connection is live.
        let cookie = unsafe { (randr.get_crtc_transform)(self.raw(), crtc) };
        let reply = self.reply("GetCrtcTransform", |error| {
            // SAFETY: the cookie is this connection's, for this request.
            unsafe { (randr.get_crtc_transform_reply)(self.raw(), cookie, error) }
        })?;
        // SAFETY: each accessor reads only the live reply's header to locate
        // its list; `list` bounds-checks what it then copies.
        let (pending_name, pending_params, current_name, current_params) = unsafe {
            (
                (randr.transform_pending_filter_name)(reply.as_ptr()),
                (randr.transform_pending_params)(reply.as_ptr()),
                (randr.transform_current_filter_name)(reply.as_ptr()),
                (randr.transform_current_params)(reply.as_ptr()),
            )
        };
        let name = |start: *mut core::ffi::c_char, len: u16| {
            reply
                .list(start.cast::<u8>().cast_const(), usize::from(len))
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        };
        Ok(CrtcTransform {
            has_transforms: reply.has_transforms != 0,
            pending: Transform::from_raw(&reply.pending_transform),
            pending_filter: name(pending_name, reply.pending_len)?,
            pending_params: reply.list(pending_params, usize::from(reply.pending_nparams))?,
            current: Transform::from_raw(&reply.current_transform),
            current_filter: name(current_name, reply.current_len)?,
            current_params: reply.list(current_params, usize::from(reply.current_nparams))?,
        })
    }

    /// `SetCrtcTransform` (RandR 1.3): the transform and filter the CRTC's
    /// next `SetCrtcConfig` applies. `params` are 16.16 fixed point.
    ///
    /// # Errors
    ///
    /// `BadValue` from a CRTC that cannot be transformed — Xvfb's — or an
    /// unknown filter, `BadRRCrtc` for one that does not exist,
    /// [`Error::TooLong`], why RandR cannot be used, or what else the request
    /// met.
    pub fn set_crtc_transform(
        &self,
        crtc: Crtc,
        transform: &Transform,
        filter: &str,
        params: &[i32],
    ) -> Result<(), Error> {
        let randr = self.randr_at(1, 3)?;
        let filter_len = u16::try_from(filter.len()).map_err(|_| Error::TooLong {
            what: "filter name",
        })?;
        let params_len = u32::try_from(params.len()).map_err(|_| Error::TooLong {
            what: "filter parameters",
        })?;
        // SAFETY: the connection is live; the filter name and parameters are
        // the lengths given and outlive the call, which copies them.
        self.check(unsafe {
            (randr.set_crtc_transform_checked)(
                self.raw(),
                crtc,
                transform.to_raw(),
                filter_len,
                filter.as_ptr().cast(),
                params_len,
                params.as_ptr(),
            )
        })
    }

    /// `SetCrtcConfig` (RandR 1.2).
    ///
    /// `timestamp` is [`ScreenResources::timestamp`] (or `0` for now) and
    /// `config_timestamp` must be [`ScreenResources::config_timestamp`]: a
    /// server that checks them answers a stale one with a
    /// [`SetConfigStatus`], not an error. See there for the X.Org server,
    /// which does not.
    ///
    /// # Errors
    ///
    /// `BadMatch` for a configuration that does not fit the screen,
    /// `BadRRCrtc`, `BadRRMode` or `BadRROutput` for ids that do not exist,
    /// [`Error::TooLong`], why RandR cannot be used, or what else the request
    /// met.
    pub fn set_crtc_config(
        &self,
        crtc: Crtc,
        timestamp: Timestamp,
        config_timestamp: Timestamp,
        config: &CrtcConfig<'_>,
    ) -> Result<SetConfigReply, Error> {
        let randr = self.randr_at(1, 2)?;
        let outputs_len =
            u32::try_from(config.outputs.len()).map_err(|_| Error::TooLong { what: "outputs" })?;
        // SAFETY: the connection is live; `outputs` is `outputs_len` ids that
        // outlive the call, which copies them.
        let cookie = unsafe {
            (randr.set_crtc_config)(
                self.raw(),
                crtc,
                timestamp,
                config_timestamp,
                config.x,
                config.y,
                config.mode,
                config.rotation.0,
                outputs_len,
                config.outputs.as_ptr(),
            )
        };
        let reply = self.reply("SetCrtcConfig", |error| {
            // SAFETY: the cookie is this connection's, for this request.
            unsafe { (randr.set_crtc_config_reply)(self.raw(), cookie, error) }
        })?;
        Ok(SetConfigReply {
            status: SetConfigStatus::from_raw(reply.status),
            timestamp: reply.timestamp,
        })
    }

    /// `SetScreenSize` (RandR 1.2): the screen's size in pixels, and the
    /// physical size it reports, in millimetres.
    ///
    /// # Errors
    ///
    /// `BadValue` for a size outside [`screen_size_range`](Self::screen_size_range),
    /// `BadMatch` for one smaller than an enabled CRTC reaches, why RandR
    /// cannot be used, or what else the request met.
    pub fn set_screen_size(
        &self,
        window: Window,
        width: u16,
        height: u16,
        mm_width: u32,
        mm_height: u32,
    ) -> Result<(), Error> {
        let randr = self.randr_at(1, 2)?;
        // SAFETY: the connection is live; the arguments are plain integers.
        self.check(unsafe {
            (randr.set_screen_size_checked)(self.raw(), window, width, height, mm_width, mm_height)
        })
    }

    /// `CreateMode` (RandR 1.2): a new mode with `mode`'s timings and name,
    /// whose id is returned. `mode.id` is ignored.
    ///
    /// The mode belongs to this connection's client and is destroyed when it
    /// disconnects, unless an output still has it.
    ///
    /// # Errors
    ///
    /// `BadName` for a name another mode has, [`Error::TooLong`], why RandR
    /// cannot be used, or what else the request met.
    pub fn create_mode(&self, window: Window, mode: &ModeInfo) -> Result<Mode, Error> {
        let randr = self.randr_at(1, 2)?;
        let raw = mode.to_raw()?;
        // SAFETY: the connection is live; the name is `raw.name_len` bytes
        // that outlive the call, which copies them.
        let cookie = unsafe {
            (randr.create_mode)(
                self.raw(),
                window,
                raw,
                u32::from(raw.name_len),
                mode.name.as_ptr().cast(),
            )
        };
        let reply = self.reply("CreateMode", |error| {
            // SAFETY: the cookie is this connection's, for this request.
            unsafe { (randr.create_mode_reply)(self.raw(), cookie, error) }
        })?;
        Ok(reply.mode)
    }

    /// `DestroyMode` (RandR 1.2).
    ///
    /// # Errors
    ///
    /// `BadAccess` for a mode an output still has, `BadRRMode` for one that
    /// does not exist, why RandR cannot be used, or what else the request
    /// met.
    pub fn destroy_mode(&self, mode: Mode) -> Result<(), Error> {
        let randr = self.randr_at(1, 2)?;
        // SAFETY: the connection is live.
        self.check(unsafe { (randr.destroy_mode_checked)(self.raw(), mode) })
    }

    /// `AddOutputMode` (RandR 1.2): lets `output` be driven at `mode`.
    ///
    /// # Errors
    ///
    /// `BadRROutput` or `BadRRMode` for ids that do not exist, why RandR
    /// cannot be used, or what else the request met.
    pub fn add_output_mode(&self, output: Output, mode: Mode) -> Result<(), Error> {
        let randr = self.randr_at(1, 2)?;
        // SAFETY: the connection is live.
        self.check(unsafe { (randr.add_output_mode_checked)(self.raw(), output, mode) })
    }

    /// `DeleteOutputMode` (RandR 1.2).
    ///
    /// # Errors
    ///
    /// `BadMatch` for a mode the output is being driven at or did not have
    /// added, why RandR cannot be used, or what else the request met.
    pub fn delete_output_mode(&self, output: Output, mode: Mode) -> Result<(), Error> {
        let randr = self.randr_at(1, 2)?;
        // SAFETY: the connection is live.
        self.check(unsafe { (randr.delete_output_mode_checked)(self.raw(), output, mode) })
    }

    /// `ListOutputProperties` (RandR 1.2).
    ///
    /// # Errors
    ///
    /// `BadRROutput` for an output that does not exist, why RandR cannot be
    /// used, or what else the request met.
    pub fn list_output_properties(&self, output: Output) -> Result<Vec<Atom>, Error> {
        let randr = self.randr_at(1, 2)?;
        // SAFETY: the connection is live.
        let cookie = unsafe { (randr.list_output_properties)(self.raw(), output) };
        let reply = self.reply("ListOutputProperties", |error| {
            // SAFETY: the cookie is this connection's, for this request.
            unsafe { (randr.list_output_properties_reply)(self.raw(), cookie, error) }
        })?;
        // SAFETY: the accessor reads only the live reply's header; `list`
        // bounds-checks what it then copies.
        let atoms = unsafe { (randr.list_output_properties_atoms)(reply.as_ptr()) };
        reply.list(atoms, usize::from(reply.num_atoms))
    }

    /// `GetOutputProperty` (RandR 1.2): up to `long_length` 4-byte units of
    /// the value, from `long_offset`. `type_` `0` accepts any type; another
    /// one that does not match returns the real type and no items. `delete`
    /// deletes the property once all of it has been read; `pending` reads the
    /// value the next `SetCrtcConfig` applies.
    ///
    /// # Errors
    ///
    /// `BadRROutput` or `BadAtom` for ids that do not exist, why RandR cannot
    /// be used, or what else the request met.
    #[expect(
        clippy::too_many_arguments,
        reason = "the request's own fields, each one a choice the caller makes"
    )]
    pub fn get_output_property(
        &self,
        output: Output,
        property: Atom,
        type_: Atom,
        long_offset: u32,
        long_length: u32,
        delete: bool,
        pending: bool,
    ) -> Result<OutputPropertyData, Error> {
        let randr = self.randr_at(1, 2)?;
        // SAFETY: the connection is live; the arguments are plain integers.
        let cookie = unsafe {
            (randr.get_output_property)(
                self.raw(),
                output,
                property,
                type_,
                long_offset,
                long_length,
                u8::from(delete),
                u8::from(pending),
            )
        };
        let reply = self.reply("GetOutputProperty", |error| {
            // SAFETY: the cookie is this connection's, for this request.
            unsafe { (randr.get_output_property_reply)(self.raw(), cookie, error) }
        })?;
        // SAFETY: the accessor reads only the live reply's header; `list`
        // bounds-checks what it then copies.
        let data = unsafe { (randr.get_output_property_data)(reply.as_ptr()) }.cast_const();
        let count = usize::try_from(reply.num_items).map_err(|_| Error::MalformedReply {
            request: "GetOutputProperty",
        })?;
        let value = match reply.format {
            0 => None,
            8 => Some(PropertyValue::Format8(reply.list(data, count)?)),
            16 => Some(PropertyValue::Format16(reply.list(data.cast(), count)?)),
            32 => Some(PropertyValue::Format32(reply.list(data.cast(), count)?)),
            _ => {
                return Err(Error::MalformedReply {
                    request: "GetOutputProperty",
                });
            }
        };
        Ok(OutputPropertyData {
            type_: reply.type_,
            bytes_after: reply.bytes_after,
            value,
        })
    }

    /// `QueryOutputProperty` (RandR 1.2).
    ///
    /// # Errors
    ///
    /// `BadName` for a property the output does not have, `BadRROutput` for
    /// an output that does not exist, why RandR cannot be used, or what else
    /// the request met.
    pub fn query_output_property(
        &self,
        output: Output,
        property: Atom,
    ) -> Result<PropertyInfo, Error> {
        let randr = self.randr_at(1, 2)?;
        // SAFETY: the connection is live.
        let cookie = unsafe { (randr.query_output_property)(self.raw(), output, property) };
        let reply = self.reply("QueryOutputProperty", |error| {
            // SAFETY: the cookie is this connection's, for this request.
            unsafe { (randr.query_output_property_reply)(self.raw(), cookie, error) }
        })?;
        // SAFETY: the accessor reads only the live reply's header; `list`
        // bounds-checks what it then copies.
        let values = unsafe { (randr.query_output_property_valid_values)(reply.as_ptr()) };
        // The reply's extra length is the value count: each is one unit.
        let count = usize::try_from(reply.length).map_err(|_| Error::MalformedReply {
            request: "QueryOutputProperty",
        })?;
        Ok(PropertyInfo {
            pending: reply.pending != 0,
            range: reply.range != 0,
            immutable: reply.immutable != 0,
            valid_values: reply.list(values, count)?,
        })
    }

    /// `ChangeOutputProperty` (RandR 1.2): sets, prepends to or appends to the
    /// property, creating it if the output does not have it. The format is
    /// `value`'s.
    ///
    /// A value too long for one request without BIG-REQUESTS makes libxcb
    /// close the connection ([`ConnectionError::RequestTooLong`]); RandR's
    /// properties are a few bytes to an EDID's few hundred.
    ///
    /// [`ConnectionError::RequestTooLong`]: crate::ConnectionError::RequestTooLong
    ///
    /// # Errors
    ///
    /// `BadValue` for a value outside the property's valid values, `BadAccess`
    /// for an immutable one, `BadMatch` for a prepend or append in another
    /// format, [`Error::TooLong`], why RandR cannot be used, or what else the
    /// request met.
    pub fn change_output_property(
        &self,
        output: Output,
        property: Atom,
        type_: Atom,
        mode: PropertyMode,
        value: &PropertyValue,
    ) -> Result<(), Error> {
        let randr = self.randr_at(1, 2)?;
        let units = u32::try_from(value.len()).map_err(|_| Error::TooLong {
            what: "property value",
        })?;
        let data: *const core::ffi::c_void = match value {
            PropertyValue::Format8(items) => items.as_ptr().cast(),
            PropertyValue::Format16(items) => items.as_ptr().cast(),
            PropertyValue::Format32(items) => items.as_ptr().cast(),
        };
        let mode = match mode {
            PropertyMode::Replace => ffi::value::PROP_MODE_REPLACE,
            PropertyMode::Prepend => PROP_MODE_PREPEND,
            PropertyMode::Append => ffi::value::PROP_MODE_APPEND,
        };
        // SAFETY: the connection is live; `data` is `units` items of
        // `value.format()` bits — the format comes from the same variant the
        // pointer and length do — and outlives the call, which copies it.
        self.check(unsafe {
            (randr.change_output_property_checked)(
                self.raw(),
                output,
                property,
                type_,
                value.format(),
                mode,
                units,
                data,
            )
        })
    }

    /// `DeleteOutputProperty` (RandR 1.2).
    ///
    /// # Errors
    ///
    /// `BadAccess` for an immutable property, `BadRROutput` for an output that
    /// does not exist, why RandR cannot be used, or what else the request met.
    pub fn delete_output_property(&self, output: Output, property: Atom) -> Result<(), Error> {
        let randr = self.randr_at(1, 2)?;
        // SAFETY: the connection is live.
        self.check(unsafe { (randr.delete_output_property_checked)(self.raw(), output, property) })
    }

    /// `SelectInput` (RandR 1.2): which RandR events `window` delivers to this
    /// connection, replacing the previous selection. They arrive through
    /// [`poll_event`](Self::poll_event).
    ///
    /// # Errors
    ///
    /// `BadWindow`, why RandR cannot be used, or what else the request met.
    pub fn select_input(&self, window: Window, mask: NotifyMask) -> Result<(), Error> {
        let randr = self.randr_at(1, 2)?;
        // SAFETY: the connection is live.
        self.check(unsafe { (randr.select_input_checked)(self.raw(), window, mask.0) })
    }
}

/// Where a resources reply's lists start: CRTCs, outputs, modes, names.
type ResourceLists = (*mut Crtc, *mut Output, *mut RandrModeInfo, *mut u8);

/// The owned value of either resources reply, its lists located by `lists`.
fn screen_resources_from(
    reply: &Reply<ffi::RandrScreenResourcesReply>,
    (crtcs, outputs, modes, names): ResourceLists,
) -> Result<ScreenResources, Error> {
    let crtcs = reply.list(crtcs, usize::from(reply.num_crtcs))?;
    let outputs = reply.list(outputs, usize::from(reply.num_outputs))?;
    let raw_modes = reply.list(modes, usize::from(reply.num_modes))?;
    let names = reply.list(names, usize::from(reply.names_len))?;
    // The names block is every mode's name back to back, in mode order.
    let mut rest = names.as_slice();
    let mut modes = Vec::with_capacity(raw_modes.len());
    for raw in raw_modes {
        let (name, tail) = rest
            .split_at_checked(usize::from(raw.name_len))
            .ok_or_else(|| reply.malformed())?;
        rest = tail;
        modes.push(ModeInfo {
            id: raw.id,
            width: raw.width,
            height: raw.height,
            dot_clock: raw.dot_clock,
            hsync_start: raw.hsync_start,
            hsync_end: raw.hsync_end,
            htotal: raw.htotal,
            hskew: raw.hskew,
            vsync_start: raw.vsync_start,
            vsync_end: raw.vsync_end,
            vtotal: raw.vtotal,
            flags: raw.mode_flags,
            name: String::from_utf8_lossy(name).into_owned(),
        });
    }
    Ok(ScreenResources {
        timestamp: reply.timestamp,
        config_timestamp: reply.config_timestamp,
        crtcs,
        outputs,
        modes,
    })
}

/// `XCB_PROP_MODE_PREPEND`, which only output properties use here.
const PROP_MODE_PREPEND: u8 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_config_statuses_map_to_their_names() {
        assert_eq!(SetConfigStatus::from_raw(0), SetConfigStatus::Success);
        assert_eq!(
            SetConfigStatus::from_raw(1),
            SetConfigStatus::InvalidConfigTime
        );
        assert_eq!(SetConfigStatus::from_raw(2), SetConfigStatus::InvalidTime);
        assert_eq!(SetConfigStatus::from_raw(3), SetConfigStatus::Failed);
        assert_eq!(SetConfigStatus::from_raw(9), SetConfigStatus::Other(9));
    }

    #[test]
    fn a_transform_round_trips_through_its_wire_form_row_by_row() {
        let transform = Transform {
            matrix: [[1, 2, 3], [4, 5, 6], [7, 8, 9]],
        };
        let raw = transform.to_raw();
        assert_eq!(raw.matrix, [1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(Transform::from_raw(&raw), transform);
        assert_eq!(Transform::default(), Transform::IDENTITY);
    }

    #[test]
    fn a_mode_name_too_long_for_its_field_is_refused() {
        let mode = ModeInfo {
            name: "x".repeat(usize::from(u16::MAX) + 1),
            ..ModeInfo::default()
        };
        assert_eq!(
            mode.to_raw().map(|_| ()),
            Err(Error::TooLong { what: "mode name" })
        );
    }

    #[test]
    fn a_property_value_reports_its_format_and_length() {
        assert_eq!(PropertyValue::Format8(vec![1, 2, 3]).format(), 8);
        assert_eq!(PropertyValue::Format16(vec![1]).len(), 1);
        assert!(PropertyValue::Format32(Vec::new()).is_empty());
    }

    #[test]
    fn rotation_bits_combine_and_are_tested_by_containment() {
        let rotations = Rotation::ROTATE_0 | Rotation::ROTATE_90 | Rotation::REFLECT_X;
        assert!(rotations.contains(Rotation::ROTATE_90));
        assert!(!rotations.contains(Rotation::ROTATE_180));
        assert_eq!(
            (NotifyMask::SCREEN_CHANGE | NotifyMask::OUTPUT_PROPERTY).0,
            0b1001
        );
    }
}
