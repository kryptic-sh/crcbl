//! App core: what a windowed program needs to get from a window to a GPU
//! device and to take typing, without the game engine.
//!
//! ```text
//! crcbl_shell::open() ──▶ window::open_window ──▶ window::wait_for_configure
//!                                                         │ extent
//!                                                         ▼
//!        backend::request_open ──▶ adapter::select ──▶ device + swapchain
//!
//! ShellEvent ──▶ text_input::TextPump ──▶ crcbl_ui's TextInput
//! ```
//!
//! [`backend`] walks the GPU backends this target has and falls through past
//! one that cannot open, [`adapter`] reads the `CRCBL_ADAPTER` pin that says
//! which device inside it, [`window`] opens a window and waits for its first
//! size, and [`text_input`] joins the shell's key, commit and clipboard events
//! to `crcbl-ui`'s text input. A program that draws only a UI — a status bar, a
//! launcher — depends on this crate, `crcbl-ui` and `crcbl-frame`, and links
//! no audio, physics, networking or ECS.
//!
//! The `crcbl` umbrella depends on this crate and re-exports every module at
//! its old path (`crcbl::backend`, `crcbl::adapter`,
//! `crcbl::text_input::TextPump`, `crcbl::engine::wait_for_configure`), so a
//! game names nothing here.
//!
//! # What is not here
//!
//! - **`GpuContext`**, the device and swapchain the engine draws through. It
//!   still lives in `crcbl::engine`: it reads the player's `[engine.video]`
//!   settings, hands the renderer its effect request and writes `--screenshot`
//!   frames, and each of those reaches a crate this one must not link. It
//!   moves here once the core presenter is split from those.
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
//! let (width, height) = wait_for_configure(&mut shell, window, &mut events)?;
//! assert!(width > 0 && height > 0);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod adapter;

pub mod backend;

pub mod text_input;

pub mod window;
