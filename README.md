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

## 当前状态

**已实现功能**：
- Socket 通信协议（裁切、变换、多屏幕控制 API）
- 环境变量传递（`WAYLAND_DISPLAY`, `XDG_SESSION_TYPE`, `MYDE_WRAP_SOCKET`）
- 屏幕信息获取
- 命令行参数解析
- 渲染缓冲区管理（软件渲染）
- DRM 设备检测和初始化
- Wayland compositor 核心框架（smithay 0.7）
- shm 缓冲区接收
- dmabuf 协议支持
- 离屏合成（支持 subsurface）
- DRM 渲染输出（dumb buffer）
- 裁切和变换渲染逻辑
- 多屏幕输出支持

**待实现**：
- GBM 缓冲区管理
- DrmCompositor 集成
- 输入事件处理

## 核心功能说明

本项目不是普通的 Wayland 全屏显示，而是支持：

1. **窗口裁切**：从应用渲染画面中截取指定矩形区域
2. **缩放变换**：对裁切区域进行缩放处理
3. **平移变换**：对裁切区域进行平移处理
4. **多屏幕输出**：将变换后的区域分别渲染到不同屏幕

## 技术架构

- **smithay** (v0.7): Wayland compositor 框架
- **wayland-server** (v0.31): Wayland 协议实现
- **drm** (v0.14): DRM 设备控制
- **gbm** (v0.18): GBM 缓冲区管理
- **wayland-protocols** (v0.32): Wayland 协议扩展

## 运行

```bash
# 以 root 权限运行（需要访问 DRM 设备）
sudo myde-wrap <command> [args...]

# 或者将用户添加到 video 组
sudo usermod -aG video $USER
# 重新登录后
myde-wrap <command> [args...]

# 示例
sudo myde-wrap alacritty
sudo myde-wrap glxgears
```

**注意**: DRM 渲染需要 root 权限或用户在 video 组中。

## 快速开始

### 编译

```bash
cargo build --release
```

### 运行

```bash
# 以 root 权限运行（需要访问 DRM 设备）
sudo myde-wrap <command> [args...]

# 或者将用户添加到 video 组
sudo usermod -aG video $USER
# 重新登录后
myde-wrap <command> [args...]

# 示例
sudo myde-wrap alacritty
sudo myde-wrap glxgears
```

**注意**: DRM 渲染需要 root 权限或用户在 video 组中。

程序启动后会：
1. 自动检测默认显示器分辨率
2. 创建 Socket 并设置环境变量 `MYDE_WRAP_SOCKET`
3. 设置 Wayland 环境变量 `WAYLAND_DISPLAY` 和 `XDG_SESSION_TYPE`
4. 继承父进程所有环境变量，确保应用能正常访问显示服务
5. 启动指定的应用程序
6. 应用立即显示在默认显示器上，裁取范围为整个窗口（分辨率匹配显示器）

### 默认行为

- **显示位置**: 默认显示器（自动检测）
- **窗口大小**: 以显示器分辨率为准（如 1920x1080）
- **裁取范围**: 整个窗口（`x=0, y=0, width=屏幕宽度, height=屏幕高度`）
- **变换**: 无变换（`scale_x=1.0, scale_y=1.0, translate_x=0, translate_y=0`）

### 环境变量

程序会自动设置以下环境变量，并继承父进程的所有环境变量：

- `MYDE_WRAP_SOCKET`: Socket 文件路径
- `WAYLAND_DISPLAY`: Wayland 显示名称（默认 `wayland-0`）
- `XDG_SESSION_TYPE`: 会话类型（默认 `wayland`）

子进程会继承所有环境变量，确保能正常访问 X11/Wayland 显示服务。

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
