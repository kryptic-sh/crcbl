//! `crcbl screenshot` — offscreen render → readback → PNG.
//!
//! The handler opens the GPU backend, renders one frame through the forward
//! renderer, reads the pixels back into host memory, and writes a PNG.

use crate::args::ScreenshotArgs;
use crate::report::{Failure, Outcome};

pub fn run(args: &ScreenshotArgs) -> Result<Outcome, Failure> {
    let mut setup = crcbl::screenshot::OffscreenSetup::open(args.width, args.height)
        .map_err(|error| Failure::new(format!("could not open GPU backend: {error}")))?;

    // One frame at an identity pose — the sandbox cube sits at the origin
    // and the camera is a fixed perspective.
    let ((width, height), pixels) = setup
        .draw_and_readback()
        .map_err(|error| Failure::new(format!("render/readback failed: {error}")))?;

    setup.finish();

    // Row-major top-row-first RGBA8 sRGB → PNG.
    let image = crcbl_golden::Image::from_rgba8(width, height, pixels)
        .map_err(|error| Failure::new(format!("pixels don't match dimensions: {error}")))?;
    image.save_png(&args.output).map_err(|error| {
        Failure::new(format!(
            "could not write {}: {error}",
            args.output.display()
        ))
    })?;

    Ok(Outcome {
        human: format!("wrote {} ({}×{})", args.output.display(), width, height),
        json: vec![],
    })
}
