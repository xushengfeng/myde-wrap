use crate::protocol::Transform;
use crate::wayland::App;
use smithay::reexports::calloop::EventLoop;

pub trait RenderBackend: Send {
    fn name(&self) -> &str;
    fn init(
        &mut self,
        event_loop: &mut EventLoop<App>,
        state: &mut App,
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn render_rect(
        &mut self,
        screen_index: usize,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        transform: &Transform,
    );
    fn dispatch(&mut self);
    fn get_width(&self) -> u32;
    fn get_height(&self) -> u32;
    fn get_output_count(&self) -> usize;

    /// Render the Wayland space contents to the screen.
    /// This is called periodically to update the display with Wayland surfaces.
    fn render_space(&mut self, _state: &mut App) {
        // Default implementation does nothing
    }

    /// Called when a frame has been submitted and the next frame can be rendered.
    /// This is used for DRM page flip synchronization.
    fn frame_submitted(&mut self) {
        // Default implementation does nothing
    }
}
