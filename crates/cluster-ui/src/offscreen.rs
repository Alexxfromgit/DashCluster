//! Headless rendering with Slint's software renderer, for snapshots and CI.
//!
//! Call [`install`] once, before any Slint component is created, in a process that
//! doesn't open real windows.

use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::{PhysicalSize, Rgb8Pixel};
use std::rc::Rc;

struct OffscreenPlatform {
    window: Rc<MinimalSoftwareWindow>,
}

impl Platform for OffscreenPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }
}

/// Handle to the single offscreen window.
pub struct Offscreen {
    window: Rc<MinimalSoftwareWindow>,
}

/// Installs the offscreen platform with a `width`×`height` physical-pixel window.
pub fn install(width: u32, height: u32) -> anyhow::Result<Offscreen> {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    window.set_size(PhysicalSize::new(width, height));
    slint::platform::set_platform(Box::new(OffscreenPlatform {
        window: window.clone(),
    }))
    .map_err(|e| anyhow::anyhow!("set platform: {e}"))?;
    Ok(Offscreen { window })
}

impl Offscreen {
    pub fn size(&self) -> PhysicalSize {
        use slint::platform::WindowAdapter as _;
        self.window.size()
    }

    /// Renders the current scene to RGB8 pixels.
    pub fn render(&self) -> Vec<Rgb8Pixel> {
        let size = self.size();
        let mut buf = vec![Rgb8Pixel::new(0, 0, 0); (size.width * size.height) as usize];
        slint::platform::update_timers_and_animations();
        self.window.request_redraw();
        self.window.draw_if_needed(|r| {
            r.render(&mut buf, size.width as usize);
        });
        buf
    }

    pub fn render_png(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let size = self.size();
        let pixels = self.render();
        let file = std::io::BufWriter::new(std::fs::File::create(path)?);
        let mut enc = png::Encoder::new(file, size.width, size.height);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header()?;
        let bytes: Vec<u8> = pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
        w.write_image_data(&bytes)?;
        Ok(())
    }
}
