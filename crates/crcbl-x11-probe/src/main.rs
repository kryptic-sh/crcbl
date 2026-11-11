//! Prints the X server's outputs, CRTCs and modes as RandR describes them.
//!
//! Connects to `DISPLAY`, or to the display named as the one argument.

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn main() -> std::process::ExitCode {
    let display = std::env::args().nth(1);
    match describe(display.as_deref()) {
        Ok(text) => {
            print!("{text}");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("crcbl-x11-probe: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// The report, built whole so a failure halfway prints nothing misleading.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn describe(display: Option<&str>) -> Result<String, Box<dyn std::error::Error>> {
    use core::fmt::Write as _;

    let connection = crcbl_x11::Connection::connect(display)?;
    let screen = connection.screen();
    let randr = connection.randr()?;
    let resources = connection.screen_resources_current(screen.root)?;
    let primary = connection.output_primary(screen.root)?;
    let mut text = String::new();
    writeln!(
        text,
        "screen {}: {}x{} px, {}x{} mm, RandR {}",
        screen.number,
        screen.width,
        screen.height,
        screen.width_mm,
        screen.height_mm,
        randr.version
    )?;
    for &output in &resources.outputs {
        let info = connection.output_info(output, resources.config_timestamp)?;
        let primary = if output == primary { " primary" } else { "" };
        writeln!(
            text,
            "output {} ({output:#x}): {:?}, crtc {:#x}, {}x{} mm{primary}",
            info.name, info.connection, info.crtc, info.mm_width, info.mm_height
        )?;
        for (index, mode) in info.modes.iter().enumerate() {
            let preferred = if index < usize::from(info.num_preferred) {
                " preferred"
            } else {
                ""
            };
            writeln!(text, "  mode {mode:#x}{preferred}")?;
        }
    }
    for &crtc in &resources.crtcs {
        let info = connection.crtc_info(crtc, resources.config_timestamp)?;
        writeln!(
            text,
            "crtc {crtc:#x}: {}x{}+{}+{}, mode {:#x}, rotation {:#x}, outputs {:x?}",
            info.width, info.height, info.x, info.y, info.mode, info.rotation.0, info.outputs
        )?;
    }
    for mode in &resources.modes {
        let pixels_per_frame = u64::from(mode.htotal) * u64::from(mode.vtotal);
        let millihertz = (u64::from(mode.dot_clock) * 1000)
            .checked_div(pixels_per_frame)
            .unwrap_or(0);
        writeln!(
            text,
            "mode {:#x} {}: {}x{} @ {millihertz} mHz",
            mode.id, mode.name, mode.width, mode.height
        )?;
    }
    Ok(text)
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn main() -> std::process::ExitCode {
    eprintln!("crcbl-x11-probe: X11 exists only on Linux and FreeBSD");
    std::process::ExitCode::FAILURE
}
