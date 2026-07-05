use crate::protocol::{Rect, ScreenInfo, Transform};
use tracing::info;

pub struct Renderer {
    window_width: u32,
    window_height: u32,
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
    pub fn new() -> Self {
        let screens = Self::detect_screens();
        let default_screen = &screens[0];
        let window_width = default_screen.width;
        let window_height = default_screen.height;

        info!(
            "默认显示器: {} ({}x{})",
            default_screen.name, window_width, window_height
        );

        let captured_rects = vec![Rect {
            x: 0,
            y: 0,
            width: window_width,
            height: window_height,
        }];

        let transforms = vec![Transform { rotation: 0.0 }];

        Self {
            window_width,
            window_height,
            captured_rects,
            transforms,
            screens,
            input_enabled: true,
            screen_configs: Vec::new(),
        }
    }

    fn detect_screens() -> Vec<ScreenInfo> {
        // 尝试从环境变量获取屏幕信息
        if let Ok(output) = std::process::Command::new("xrandr").arg("--query").output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut screens = Vec::new();

            for line in stdout.lines() {
                if line.contains(" connected") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    let name = parts[0].to_string();

                    // 查找分辨率信息，如 "1920x1080+0+0"
                    for part in &parts[2..] {
                        if part.contains('x') && part.contains('+') {
                            let res_part = part.split('+').next().unwrap_or("");
                            let dims: Vec<&str> = res_part.split('x').collect();
                            if dims.len() == 2 {
                                if let (Ok(width), Ok(height)) =
                                    (dims[0].parse::<u32>(), dims[1].parse::<u32>())
                                {
                                    screens.push(ScreenInfo {
                                        name,
                                        width,
                                        height,
                                        refresh_rate: 60,
                                    });
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            if !screens.is_empty() {
                return screens;
            }
        }

        // 如果无法检测，返回默认屏幕
        info!("无法检测屏幕信息，使用默认配置");
        vec![ScreenInfo {
            name: "Screen-0".to_string(),
            width: 1920,
            height: 1080,
            refresh_rate: 60,
        }]
    }

    pub fn set_window_size(&mut self, width: u32, height: u32) {
        self.window_width = width;
        self.window_height = height;
    }

    #[allow(dead_code)]
    pub fn get_window_size(&self) -> (u32, u32) {
        (self.window_width, self.window_height)
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

    // 获取默认的全屏配置
    pub fn get_default_fullscreen_config(&self, screen_index: usize) -> ScreenConfig {
        if screen_index >= self.screens.len() {
            return ScreenConfig {
                screen_index,
                rects: vec![Rect {
                    x: 0,
                    y: 0,
                    width: self.window_width,
                    height: self.window_height,
                }],
                transforms: vec![Transform { rotation: 0.0 }],
            };
        }

        let screen = &self.screens[screen_index];

        ScreenConfig {
            screen_index,
            rects: vec![Rect {
                x: 0,
                y: 0,
                width: self.window_width,
                height: self.window_height,
            }],
            transforms: vec![Transform { rotation: 0.0 }],
        }
    }

    // 计算变换后的矩形（为了兼容旧接口，这里直接返回原矩形，因为拉伸由渲染器在渲染时负责）
    pub fn compute_transformed_rect(rect: &Rect, _transform: &Transform) -> (i32, i32, u32, u32) {
        (rect.x, rect.y, rect.width, rect.height)
    }
}
