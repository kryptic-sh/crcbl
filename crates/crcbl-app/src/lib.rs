//! App core: what a windowed program needs to get from a window to a GPU
//! device and to take typing, without the game engine.
//!
//! ```text
//! crcbl_shell::open() ──▶ window::open_window ──▶ window::wait_for_configure
//!                                                         │ extent
//!                                                         ▼
//!   present::Presenter: backend::request_open ──▶ adapter::choose
//!                       ──▶ device + swapchain ──▶ acquire / submit_and_present
//!
//! ShellEvent ──▶ text_input::TextPump ──▶ crcbl_ui's TextInput
//! ```
//!
//! [`backend`] walks the GPU backends this target has and falls through past
//! one that cannot open, [`adapter`] decides which device inside it — the
//! `CRCBL_ADAPTER` pin, the program's pick, or a ranking by power preference
//! — [`window`] opens a window and waits for its first size, [`present`]
//! opens a device and a swapchain on that window and paces frames into it,
//! and [`text_input`] joins the shell's key, commit and clipboard events to
//! `crcbl-ui`'s text input. A program that draws only a
//! UI — a status bar, a launcher — depends on this crate, `crcbl-ui` and
//! `crcbl-frame`, opens a [`present::Presenter`] where a game opens the
//! engine's `GpuContext`, and links no audio, physics, networking or ECS.
//! `crates/crcbl-app/examples/ui_window.rs` draws one frame that way.
//!
//! The `crcbl` umbrella depends on this crate and re-exports every module at
//! its old path (`crcbl::backend`, `crcbl::adapter`,
//! `crcbl::text_input::TextPump`, `crcbl::engine::wait_for_configure`,
//! `crcbl::engine::Pacing`), so a game names nothing here.
//!
//! # What is not here
//!
//! - **`GpuContext`'s engine half.** `crcbl::engine::GpuContext` wraps a
//!   [`present::Presenter`] and adds the player's `[engine.video]` settings,
//!   the renderer's effect request and `--screenshot` frames, and each of
//!   those reaches a crate this one must not link.
//! - **The frame loop, the clock and the menus**, which are the engine's.
//!   [`window::open_window`] takes the clock's elapsed time rather than the
//!   engine's `Clock`, and `crcbl::engine::open_window` is the wrapper that
//!   passes it.
//! - **The renderer.** `crcbl-frame` is the render graph and the UI pass, and
//!   it stays usable offscreen with no window and no backend, which is why
//!   none of this went there.
//!
//! # Example
//!
//! ```
//! use crcbl_app::window::{open_window, wait_for_configure};
//! use crcbl_shell::{HeadlessShell, WindowDesc};
//! use std::time::Duration;
//!
//! let mut shell = HeadlessShell::new();
//! let window = open_window(&mut shell, Duration::ZERO, &WindowDesc::default())?;
//! let mut events = 0;
//! let (width, height) = wait_for_configure(&mut shell, window, &mut |_| events += 1)?;
//! assert!(width > 0 && height > 0);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod adapter;

pub mod backend;

pub mod present;

pub mod text_input;

pub mod window;
