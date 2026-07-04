use smithay::reexports::calloop::EventLoop;
use crate::wayland::App;
use crate::protocol::{Rect, Transform};

pub trait RenderBackend {
    fn name(&self) -> &str;
    fn init(&mut self, event_loop: &mut EventLoop<App>, state: &mut App) -> Result<(), Box<dyn std::error::Error>>;
    fn render_rect(&mut self, screen_index: usize, x: i32, y: i32, width: u32, height: u32, transform: &Transform);
    fn dispatch(&mut self);
    fn get_width(&self) -> u32;
    fn get_height(&self) -> u32;
    fn get_output_count(&self) -> usize;
}
