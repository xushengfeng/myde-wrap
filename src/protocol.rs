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
/// 变换顺序：先缩放，后平移
/// 缩放原点：矩形左上角 (rect.x, rect.y)
///
/// 变换公式：
///   final_x = rect.x + (rect.x * scale_x - rect.x) + translate_x
///           = rect.x * scale_x + translate_x
///   final_y = rect.y + (rect.y * scale_y - rect.y) + translate_y
///           = rect.y * scale_y + translate_y
///   final_width = rect.width * scale_x
///   final_height = rect.height * scale_y
///
/// 示例：
///   - scale_x=2.0, scale_y=2.0: 矩形以左上角为原点放大2倍
///   - translate_x=100, translate_y=50: 矩形向右移动100像素，向下移动50像素
///   - scale_x=0.5, scale_y=0.5, translate_x=200: 矩形缩小一半，然后向右移动200像素
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transform {
    /// X 轴缩放因子（1.0 = 不缩放，2.0 = 放大2倍，0.5 = 缩小一半）
    pub scale_x: f64,
    /// Y 轴缩放因子（1.0 = 不缩放，2.0 = 放大2倍，0.5 = 缩小一半）
    pub scale_y: f64,
    /// X 轴平移量（像素，正值向右）
    pub translate_x: f64,
    /// Y 轴平移量（像素，正值向下）
    pub translate_y: f64,
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
    SetWindowSize { width: u32, height: u32 },
    CaptureRects { rects: Vec<Rect> },
    TransformRects { transforms: Vec<Transform> },
    RenderToScreen { screen_index: usize, rects: Vec<Rect>, transforms: Vec<Transform> },
    GetScreens,
    SetInputEnabled { enabled: bool },
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
