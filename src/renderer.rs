use crate::protocol::{Rect, ScreenInfo, Transform};
use tracing::info;

pub struct Renderer {
    /// SetWindowSize 声明的虚拟画布尺寸（可突破物理屏幕）
    canvas_size: Option<(u32, u32)>,
    captured_rects: Vec<Rect>,
    transforms: Vec<Transform>,
    screens: Vec<ScreenInfo>,
    input_enabled: bool,
    screen_configs: Vec<ScreenConfig>,
}

#[derive(Clone, Debug)]
pub struct ScreenConfig {
    pub screen_index: usize,
    pub rects: Vec<Rect>,
    pub transforms: Vec<Transform>,
}

impl Renderer {
    /// `screens` 应传入渲染后端的真实输出信息
    pub fn new(screens: Vec<ScreenInfo>) -> Self {
        info!("screens: {:?}", screens);
        if screens.is_empty() {
            info!("no output detected, using auto canvas");
        }

        Self {
            canvas_size: None,
            captured_rects: Vec::new(),
            transforms: vec![Transform { rotation: 0.0 }],
            screens,
            input_enabled: true,
            screen_configs: Vec::new(),
        }
    }

    /// SetWindowSize：声明虚拟画布尺寸（不强制修改应用窗口大小，
    /// 应用窗口由应用自行控制）。实际画布 = max(声明尺寸, 应用窗口尺寸)，
    /// 保证虚拟画布不裁切应用。
    pub fn set_window_size(&mut self, width: u32, height: u32) {
        self.canvas_size = Some((width, height));
    }

    /// 获取 SetWindowSize 声明的虚拟画布尺寸（None 表示未声明，自动跟随应用窗口）
    pub fn get_canvas_size(&self) -> Option<(u32, u32)> {
        self.canvas_size
    }

    pub fn capture_rects(&mut self, rects: Vec<Rect>) -> Vec<Rect> {
        self.captured_rects = rects.clone();
        rects
    }

    pub fn transform_rects(&mut self, transforms: Vec<Transform>) {
        self.transforms = transforms;
    }

    pub fn render_to_screen(
        &mut self,
        screen_index: usize,
        rects: Vec<Rect>,
        transforms: Vec<Transform>,
    ) -> anyhow::Result<()> {
        if screen_index >= self.screens.len() {
            return Err(anyhow::anyhow!("Invalid screen index: {}", screen_index));
        }

        // 保存屏幕配置
        let config = ScreenConfig {
            screen_index,
            rects: rects.clone(),
            transforms: transforms.clone(),
        };

        // 更新或添加屏幕配置
        if let Some(existing) = self
            .screen_configs
            .iter_mut()
            .find(|c| c.screen_index == screen_index)
        {
            *existing = config;
        } else {
            self.screen_configs.push(config);
        }

        self.captured_rects = rects;
        self.transforms = transforms;
        Ok(())
    }

    pub fn get_screens(&self) -> Vec<ScreenInfo> {
        self.screens.clone()
    }

    pub fn set_input_enabled(&mut self, enabled: bool) {
        self.input_enabled = enabled;
    }

    #[allow(dead_code)]
    pub fn is_input_enabled(&self) -> bool {
        self.input_enabled
    }

    #[allow(dead_code)]
    pub fn get_captured_rects(&self) -> &[Rect] {
        &self.captured_rects
    }

    #[allow(dead_code)]
    pub fn get_transforms(&self) -> &[Transform] {
        &self.transforms
    }

    pub fn get_screen_configs(&self) -> &[ScreenConfig] {
        &self.screen_configs
    }

    // 获取默认的全屏配置：rects 留空表示"整个画布"（自动取
    // max(SetWindowSize 声明尺寸, 应用窗口尺寸)），拉伸铺满屏幕
    pub fn get_default_fullscreen_config(&self, screen_index: usize) -> ScreenConfig {
        ScreenConfig {
            screen_index,
            rects: Vec::new(),
            transforms: vec![Transform { rotation: 0.0 }],
        }
    }

    // 计算变换后的矩形（为了兼容旧接口，这里直接返回原矩形，因为拉伸由渲染器在渲染时负责）
    pub fn compute_transformed_rect(rect: &Rect, _transform: &Transform) -> (i32, i32, u32, u32) {
        (rect.x, rect.y, rect.width, rect.height)
    }
}
