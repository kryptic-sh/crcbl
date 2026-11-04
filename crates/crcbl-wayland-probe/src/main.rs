//! Prints the compositor's outputs as `zwlr_output_manager_v1` describes them.

#[cfg(desktop_unix)]
fn main() -> std::process::ExitCode {
    match crcbl_wayland_probe::read_heads() {
        Ok(heads) => {
            for head in heads {
                let state = if head.enabled { "enabled" } else { "disabled" };
                println!("{} ({state}): {}", head.name, head.description);
                for (index, mode) in head.modes.iter().enumerate() {
                    let current = if head.current_mode == Some(index) {
                        " current"
                    } else {
                        ""
                    };
                    let preferred = if mode.preferred { " preferred" } else { "" };
                    println!(
                        "  {}x{} @ {} mHz{current}{preferred}",
                        mode.width, mode.height, mode.refresh_mhz
                    );
                }
            }
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("crcbl-wayland-probe: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(desktop_unix))]
fn main() -> std::process::ExitCode {
    eprintln!("crcbl-wayland-probe: Wayland exists only on Linux and FreeBSD");
    std::process::ExitCode::FAILURE
}
