use crate::protocol::ScreenInfo;
use crate::renderer::ScreenConfig;
use crate::wayland::App;
use smithay::reexports::calloop::EventLoop;

pub trait RenderBackend: Send {
    fn name(&self) -> &str;

    /// 返回真实输出屏幕信息（供 GetScreens 使用）
    fn get_screens(&self) -> Vec<ScreenInfo>;
    fn init(
        &mut self,
        event_loop: &mut EventLoop<App>,
        state: &mut App,
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn dispatch(&mut self);
    fn get_output_count(&self) -> usize;

    /// Render the Wayland space contents to the screen.
    /// This is called periodically to update the display with Wayland surfaces.
    ///
    /// `canvas_size` 为 SetWindowSize 声明的虚拟画布尺寸（可为 None）。
    /// 实际画布 = max(声明尺寸, 应用窗口尺寸)；配置中 rects 留空（或无配置）时，
    /// 默认把整个画布拉伸铺满目标屏幕。
    fn render_space(
        &mut self,
        _state: &mut App,
        _configs: &[ScreenConfig],
        _canvas_size: Option<(u32, u32)>,
    ) {
        // Default implementation does nothing
    }
}
