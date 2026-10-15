//! The engine's UI pass: `crcbl_frame`'s, with the engine's menu art in its
//! image atlas from the first frame.
//!
//! [`crcbl_frame::ui_pass`] draws a [`DrawList`] with whatever pictures its
//! caller registered, and knows nothing of the engine's menu. [`UiRenderer`] here is that pass plus the shipped menu skin
//! ([`menu_skin`](crate::menu::menu_skin)), registered at start-up so the art is
//! on the page before a menu is ever opened — which is what every game's pause
//! menu, the debug panel and the console draw with. Every other method forwards
//! to the frame core's renderer, so a caller writes `renderer.begin_frame(…)`
//! and `renderer.add_passes(…)` as it always has.

use crcbl_frame::counters::FrameCounters;
use crcbl_frame::graph::{ImageId, RenderGraph};
use crcbl_hal::{Device, Format, HalError, QueueHandle};
use crcbl_ui::draw_list::DrawList;
use crcbl_ui::font::atlas::GlyphAtlas;
use crcbl_ui::image::ImageAtlas;
use crcbl_ui::menu::MenuSkin;
use crcbl_ui::text::FontAtlas;

pub use crcbl_frame::ui_pass::{
    CONSTANTS_BINDING, GLYPH_PAGES_BINDING, IMAGE_ATLAS_BINDING, IMAGE_SAMPLER_BINDING,
    TEXTURE_SET, UiTexture,
};

/// The UI compositing renderer, with the engine's menu art registered.
#[derive(Debug)]
pub struct UiRenderer {
    pass: crcbl_frame::UiRenderer,
    /// The shipped menu art, registered in the pass's image atlas at start-up.
    menu_skin: MenuSkin,
}

impl UiRenderer {
    /// The most passes a frame of this renderer records:
    /// [`crcbl_frame::UiRenderer::MAX_PASSES`], since the menu art adds none.
    pub const MAX_PASSES: u32 = crcbl_frame::UiRenderer::MAX_PASSES;

    /// Builds the UI pass with the menu art on its image page.
    ///
    /// `target_format` is the format of the image the pass composites onto, as
    /// [`crcbl_frame::UiRenderer::new`] describes.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call, with everything already created
    /// released, and [`HalError::InvalidDescriptor`] if the menu art does not
    /// fit an empty image page — a shipped-art defect, not a device one.
    pub fn new(
        device: &dyn Device,
        queue: QueueHandle,
        target_format: Format,
    ) -> Result<Self, HalError> {
        let mut images = ImageAtlas::new();
        let menu_skin = crate::menu::menu_skin(&mut images).map_err(|error| {
            HalError::InvalidDescriptor(format!("ui image atlas: the menu art: {error}"))
        })?;
        let pass = crcbl_frame::UiRenderer::with_images(device, queue, target_format, images)?;
        Ok(Self { pass, menu_skin })
    }

    /// The shipped menu art, for [`Menu::render`](crcbl_ui::menu::Menu::render).
    #[must_use]
    pub const fn menu_skin(&self) -> &MenuSkin {
        &self.menu_skin
    }

    /// The format the compositing pass renders into.
    #[must_use]
    pub const fn target_format(&self) -> Format {
        self.pass.target_format()
    }

    /// The image atlas the pass draws pictures from, the menu art among them.
    #[must_use]
    pub const fn images(&self) -> &ImageAtlas {
        self.pass.images()
    }

    /// The image atlas, to register pictures into, as
    /// [`crcbl_frame::UiRenderer::images_mut`] describes.
    pub const fn images_mut(&mut self) -> &mut ImageAtlas {
        self.pass.images_mut()
    }

    /// The glyph atlas the pass rasterises text into.
    #[must_use]
    pub const fn glyphs(&self) -> &GlyphAtlas {
        self.pass.glyphs()
    }

    /// Uploads the frame's draw list, as
    /// [`crcbl_frame::UiRenderer::begin_frame`] does.
    ///
    /// # Errors
    ///
    /// As [`crcbl_frame::UiRenderer::begin_frame`].
    pub fn begin_frame(
        &mut self,
        device: &dyn Device,
        draw_list: &DrawList,
        atlas: &FontAtlas,
        scale: f32,
    ) -> Result<(), HalError> {
        self.pass.begin_frame(device, draw_list, atlas, scale)
    }

    /// What the last frame recorded, as [`crcbl_frame::UiRenderer::counters`]
    /// reports it.
    #[must_use]
    pub fn counters(&self) -> FrameCounters {
        self.pass.counters()
    }

    /// Adds the frame's passes to `graph`, as
    /// [`crcbl_frame::UiRenderer::add_passes`] does.
    pub fn add_passes<'a>(
        &'a self,
        graph: &mut RenderGraph<'a>,
        target: ImageId,
        extent: (u32, u32),
    ) {
        self.pass.add_passes(graph, target, extent);
    }

    /// Adds the frame's passes with renderer-drawn textures bound, as
    /// [`crcbl_frame::UiRenderer::add_passes_with_textures`] does.
    pub fn add_passes_with_textures<'a>(
        &'a self,
        graph: &mut RenderGraph<'a>,
        target: ImageId,
        extent: (u32, u32),
        textures: &[UiTexture],
    ) {
        self.pass
            .add_passes_with_textures(graph, target, extent, textures);
    }

    /// Destroys the pass's GPU resources, as
    /// [`crcbl_frame::UiRenderer::destroy`] does.
    pub fn destroy(self, device: &dyn Device) {
        self.pass.destroy(device);
    }
}

#[cfg(test)]
mod tests {
    use crcbl_hal::null::{Event, NullInstance, Recorder};
    use crcbl_hal::{DeviceDesc, Format, Instance, QueueKind};
    use crcbl_ui::image::ImageAtlas;
    use crcbl_ui::text::FontAtlas;

    use super::UiRenderer;

    /// **The menu art is on the page from construction**: start-up writes the
    /// built-in font's bitmap and the menu art's rectangle, and the skin it
    /// hands back names images already in the atlas.
    #[test]
    fn construction_registers_and_uploads_the_menu_art() {
        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");

        let renderer =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");

        let mut images = ImageAtlas::new();
        crate::menu::menu_skin(&mut images).expect("the menu art fits an empty page");
        let rect = images
            .dirty()
            .expect("registering the menu art dirtied the page");
        let art = rect.width as usize * rect.height as usize * 4;
        let (_, _, font) = FontAtlas::built_in().glyph_bitmap();
        let written: usize = recorder
            .events()
            .into_iter()
            .filter_map(|event| match event {
                Event::BufferWritten { len, .. } => Some(len),
                _ => None,
            })
            .sum();
        assert_eq!(
            written,
            font.len() + art,
            "the font bitmap is {} bytes and the menu art {art}",
            font.len()
        );
        assert_eq!(renderer.images().dirty(), None, "the page owes nothing");
        assert!(
            renderer
                .images()
                .images()
                .contains(&renderer.menu_skin().scrim),
            "the skin names an image the atlas holds"
        );
        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }
}
