# myde-wrap

一个轻量级的 Rust Wayland 桌面合成器，专注于渲染单个应用，并通过 Unix Socket 提供远程控制能力。

## 功能特性

- **单应用渲染**: 专注于渲染一个应用，提供高效的渲染性能
- **自动检测显示器**: 启动时自动检测默认显示器分辨率，应用全屏显示
- **默认行为**: 应用启动后立即显示在默认显示器上，裁取范围为整个窗口，无需额外配置
- **Socket 远程控制**: 通过 Unix Socket 实现远程控制，支持以下功能：
  - 控制渲染窗口大小（应用全屏）
  - 截取渲染画面中的多个矩形区域
  - 对截取的矩形进行缩放、平移变换
  - 将处理后的矩形分别渲染到多个屏幕
  - 获取屏幕参数信息
  - 开启/关闭输入功能

## 快速开始

### 编译

```bash
cargo build --release
```

### 运行

```bash
# 运行指定程序
myde-wrap <command> [args...]

# 示例
myde-wrap firefox
myde-wrap alacritty
myde-wrap glxgears
```

程序启动后会：
1. 自动检测默认显示器分辨率
2. 创建 Socket 并设置环境变量 `MYDE_WRAP_SOCKET`
3. 启动指定的应用程序
4. 应用立即显示在默认显示器上，裁取范围为整个窗口（分辨率匹配显示器）

### 默认行为

- **显示位置**: 默认显示器（自动检测）
- **窗口大小**: 以显示器分辨率为准（如 1920x1080）
- **裁取范围**: 整个窗口（`x=0, y=0, width=屏幕宽度, height=屏幕高度`）
- **变换**: 无变换（`scale_x=1.0, scale_y=1.0, translate_x=0, translate_y=0`）

### 连接 Socket

客户端可以通过环境变量获取 Socket 路径：

```rust
let socket_path = std::env::var("MYDE_WRAP_SOCKET").unwrap();
```

## Socket 协议

### 消息格式

```
[4字节大端长度][JSON消息体]
```

- 长度字段: 32位无符号整数，大端序，表示后续 JSON 消息的字节长度
- 消息体: JSON 格式的请求或响应

### 请求消息 (Client -> Server)

#### 设置窗口大小
```json
{
    "type": "SetWindowSize",
    "width": 1920,
    "height": 1080
}
```

#### 截取矩形区域
```json
{
    "type": "CaptureRects",
    "rects": [
        {"x": 0, "y": 0, "width": 100, "height": 100},
        {"x": 200, "y": 200, "width": 150, "height": 150}
    ]
}
```

#### 变换矩形

**变换顺序**: 先缩放，后平移

**缩放原点**: 矩形左上角 `(rect.x, rect.y)`

**变换公式**:
```
final_x = rect.x * scale_x + translate_x
final_y = rect.y * scale_y + translate_y
final_width = rect.width * scale_x
final_height = rect.height * scale_y
```

**参数说明**:
- `scale_x`: X 轴缩放因子（1.0 = 不缩放，2.0 = 放大2倍，0.5 = 缩小一半）
- `scale_y`: Y 轴缩放因子（1.0 = 不缩放，2.0 = 放大2倍，0.5 = 缩小一半）
- `translate_x`: X 轴平移量（像素，正值向右）
- `translate_y`: Y 轴平移量（像素，正值向下）

**示例**:
```json
{
    "type": "TransformRects",
    "transforms": [
        {"scale_x": 1.5, "scale_y": 1.5, "translate_x": 10.0, "translate_y": 20.0}
    ]
}
```
上述示例表示：以矩形左上角为原点放大1.5倍，然后向右移动10像素，向下移动20像素。

#### 渲染到屏幕
```json
{
    "type": "RenderToScreen",
    "screen_index": 0,
    "rects": [{"x": 0, "y": 0, "width": 100, "height": 100}],
    "transforms": [{"scale_x": 1.0, "scale_y": 1.0, "translate_x": 0.0, "translate_y": 0.0}]
}
```

#### 获取屏幕信息
```json
{
    "type": "GetScreens"
}
```

#### 设置输入状态
```json
{
    "type": "SetInputEnabled",
    "enabled": true
}
```

#### 心跳检测
```json
{
    "type": "Ping"
}
```

### 响应消息 (Server -> Client)

#### 窗口大小已设置
```json
{
    "type": "WindowSizeSet",
    "width": 1920,
    "height": 1080
}
```

#### 矩形已截取
```json
{
    "type": "RectsCaptured",
    "rects": [...]
}
```

#### 矩形已变换
```json
{
    "type": "RectsTransformed"
}
```

#### 已渲染到屏幕
```json
{
    "type": "RenderedToScreen",
    "screen_index": 0
}
```

#### 屏幕信息
```json
{
    "type": "Screens",
    "screens": [
        {
            "name": "Screen-0",
            "width": 1920,
            "height": 1080,
            "refresh_rate": 60
        }
    ]
}
```

#### 输入状态
```json
{
    "type": "InputState",
    "enabled": true
}
```

#### 心跳响应
```json
{
    "type": "Pong"
}
```

#### 错误响应
```json
{
    "type": "Error",
    "message": "错误描述"
}
```

## 示例客户端代码

```rust
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

fn send_message(stream: &mut UnixStream, msg: &str) -> anyhow::Result<String> {
    let len = msg.len() as u32;
    stream.write_all(&len.to_be_bytes())?;
    stream.write_all(msg.as_bytes())?;
    stream.flush()?;

    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;

    let mut msg_buf = vec![0u8; len];
    stream.read_exact(&mut msg_buf)?;

    Ok(String::from_utf8(msg_buf)?)
}

fn main() -> anyhow::Result<()> {
    let socket_path = std::env::var("MYDE_WRAP_SOCKET")?;
    let mut stream = UnixStream::connect(socket_path)?;

    // 获取屏幕信息
    let response = send_message(&mut stream, r#"{"type": "GetScreens"}"#)?;
    println!("Screens: {}", response);

    // 设置窗口大小
    let response = send_message(&mut stream, r#"{"type": "SetWindowSize", "width": 1920, "height": 1080}"#)?;
    println!("Set window size: {}", response);

    Ok(())
}
```

## 依赖

- `smithay`: Wayland 合成器框架
- `tokio`: 异步运行时
- `serde` / `serde_json`: JSON 序列化
- `tracing`: 日志追踪

## 许可证

MIT
