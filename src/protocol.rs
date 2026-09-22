use serde::{Deserialize, Serialize};

/// 矩形区域定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rect {
    /// 左上角 x 坐标（相对于窗口左上角）
    pub x: i32,
    /// 左上角 y 坐标（相对于窗口左上角）
    pub y: i32,
    /// 矩形宽度
    pub width: u32,
    /// 矩形高度
    pub height: u32,
}

/// 变换参数定义
///
/// 旋转作用于**截取**阶段：截取区域以 (rect.x, rect.y) 为锚点、绕锚点旋转 rotation 度，
/// 长宽为 rect.width × rect.height；截取结果转正后拉伸填充到目标屏幕
///（隐式缩放：scale = 屏幕尺寸 / 截取长宽），并保持直角。
///
/// 等价于：把画布绕 (rect.x, rect.y) 旋转 -rotation 度后，
/// 截取以锚点为左上角的 rect.width × rect.height 区域，拉伸铺满目标屏幕。
///
/// `rects` 与 `transforms` 按下标配对：rects[i] 对应 transforms[i]（缺省 rotation = 0）。
/// 同一屏幕配置多个区域时按数组顺序叠放，靠前的在下层。
///
/// 示例（rect = {x: 0, y: 0, width: 800, height: 600}）：
///   - rotation=0: 截取 (0, 0, 800, 600)，拉伸铺满屏幕
///   - rotation=15: 区域绕 (0, 0) 偏转 15° 截取（显示内容逆时针偏转），转正后拉伸铺满屏幕
///   - rotation=90: 截取竖条区域，转正后拉伸铺满屏幕
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transform {
    /// 截取区域相对画布的旋转角度（度）
    pub rotation: f64,
}

impl Default for Transform {
    fn default() -> Self {
        Self { rotation: 0.0 }
    }
}

/// 屏幕信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenInfo {
    /// 屏幕名称（如 "Screen-0"、"eDP-1"、"HDMI-1"）
    pub name: String,
    /// 屏幕宽度（像素）
    pub width: u32,
    /// 屏幕高度（像素）
    pub height: u32,
    /// 刷新率（Hz）
    pub refresh_rate: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    SetWindowSize {
        width: u32,
        height: u32,
    },
    CaptureRects {
        rects: Vec<Rect>,
    },
    TransformRects {
        transforms: Vec<Transform>,
    },
    /// 将画布截取区域渲染到屏幕
    ///
    /// - `rects[i]` 与 `transforms[i]` 按下标配对（缺省 rotation = 0）
    /// - 每个区域以 (x, y) 为锚点旋转截取，转正后拉伸填充到 `screen_index` 屏幕
    /// - `rects` 留空表示整个画布
    /// - 同一屏幕多个区域按数组顺序叠放，靠前的在下层
    RenderToScreen {
        screen_index: usize,
        rects: Vec<Rect>,
        transforms: Vec<Transform>,
    },
    GetScreens,
    SetInputEnabled {
        enabled: bool,
    },
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerResponse {
    WindowSizeSet { width: u32, height: u32 },
    RectsCaptured { rects: Vec<Rect> },
    RectsTransformed,
    RenderedToScreen { screen_index: usize },
    Screens { screens: Vec<ScreenInfo> },
    InputState { enabled: bool },
    Pong,
    Error { message: String },
}
