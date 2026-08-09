//! `crcbl screenshot` — offscreen render → readback → PNG.
//!
//! The handler opens the GPU backend, renders one frame of the scene `--scene`
//! named, reads the pixels back into host memory, and writes a PNG.
//!
//! # The readback is not RGBA
//!
//! `OffscreenSetup` asks the surface for its preferred format, which on every
//! ordinary desktop surface is `Bgra8UnormSrgb`, and `draw_and_readback` copies
//! the swapchain image's bytes *raw*. Handing those to `Image::from_rgba8`
//! writes a PNG with red and blue swapped — and swapped in a way a structural
//! comparison cannot see, because SSIM is computed on luma and a red/green or
//! red/blue swap barely moves it. So the format is read off the setup and
//! turned into a [`ChannelOrder`], and `Image::from_readback` does the swizzle
//! that `crcbl-golden` has had for exactly this since it was written.

use crcbl::hal::Format;
use crcbl_golden::ChannelOrder;

use crate::args::ScreenshotArgs;
use crate::json::Json;
use crate::report::{Failure, Outcome};

pub fn run(args: &ScreenshotArgs) -> Result<Outcome, Failure> {
    let mut setup = crcbl::screenshot::OffscreenSetup::open(args.width, args.height, args.scene)
        .map_err(|error| Failure::new(format!("could not open GPU backend: {error}")))?;

    let order = channel_order(setup.format());

    // One frame at an identity pose — every scene is fixed at `t = 0`, because
    // a screenshot is a golden-image input.
    let ((width, height), pixels) = setup
        .draw_and_readback()
        .map_err(|error| Failure::new(format!("render/readback failed: {error}")))?;

    // Torn down *before* the PNG is written: a device lost during the frame is
    // only observable here, and pixels read back from a lost device are not
    // something to save as a golden image.
    setup
        .finish()
        .map_err(|error| Failure::new(format!("GPU teardown failed: {error}")))?;

    // Row-major top-row-first, in the swapchain's channel order → RGBA8 → PNG.
    let image = crcbl_golden::Image::from_readback(width, height, &pixels, order)
        .map_err(|error| Failure::new(format!("pixels don't match dimensions: {error}")))?;
    image.save_png(&args.output).map_err(|error| {
        Failure::new(format!(
            "could not write {}: {error}",
            args.output.display()
        ))
    })?;

    Ok(Outcome {
        human: format!("wrote {} ({}×{})", args.output.display(), width, height),
        // The `--json` mirror of the human line. This is the one subcommand a
        // CI job reads a *path* out of, and it used to emit nothing but
        // `{"ok":true,"command":"screenshot"}`.
        json: vec![
            ("path", Json::string(args.output.display().to_string())),
            ("width", Json::Number(i64::from(width))),
            ("height", Json::Number(i64::from(height))),
        ],
    })
}

/// The channel order a readback in `format` arrives in.
///
/// Named after the memory order, like the HAL format itself, so the mapping is
/// a rename rather than a judgement.
fn channel_order(format: Format) -> ChannelOrder {
    match format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => ChannelOrder::Bgra,
        _ => ChannelOrder::Rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug this mapping exists to stop: every BGRA surface used to produce
    /// a red/blue-swapped PNG.
    #[test]
    fn bgra_surfaces_are_swizzled_and_rgba_ones_are_not() {
        assert_eq!(channel_order(Format::Bgra8UnormSrgb), ChannelOrder::Bgra);
        assert_eq!(channel_order(Format::Bgra8Unorm), ChannelOrder::Bgra);
        assert_eq!(channel_order(Format::Rgba8UnormSrgb), ChannelOrder::Rgba);
        assert_eq!(channel_order(Format::Rgba8Unorm), ChannelOrder::Rgba);
    }

    /// End to end on the byte level: a BGRA readback of a known colour has to
    /// come out of the image the way it went into the framebuffer.
    #[test]
    fn a_bgra_readback_round_trips_to_the_right_colour() {
        // Blue, as a BGRA surface stores it.
        let readback = [255u8, 0, 0, 255];
        let image = crcbl_golden::Image::from_readback(
            1,
            1,
            &readback,
            channel_order(Format::Bgra8UnormSrgb),
        )
        .expect("one pixel");
        assert_eq!(image.pixel(0, 0), Some([0, 0, 255, 255]), "red and blue");
    }
}
