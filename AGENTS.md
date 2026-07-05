# AGENTS.md

## 项目概述

myde-wrap 是一个轻量级的 Rust Wayland 桌面合成器，专注于渲染单个应用，并通过 Unix Socket 提供控制裁取缩放平移并指定到屏幕的能力。

通过 Unix Socket 实现显示控制，支持以下功能：

控制渲染窗口大小（应用全屏），可以突破物理尺寸，我们叫**原始画布**。只能有一个应用、一个窗口，这个窗口就是原始画布。

- 截取原始画布中的多个矩形区域
- 对截取的矩形进行缩放、平移变换
- 将处理后的矩形分别渲染到多个屏幕
- 获取屏幕参数信息
- 开启/关闭输入功能

提供了更灵活的kiosk模式，应用可自由操纵屏幕渲染

还有个winit窗口后端，可以显示在如kde上作为一个窗口，方便调试wayland相关

wayland协议仅窗口显示相关，不需要其他过多协议

## 技术架构

### 核心模块

```
src/
├── main.rs          # 程序入口，命令行解析，后端选择
├── protocol.rs      # 协议数据结构定义
├── socket.rs        # Unix Socket 服务端实现
├── compositor.rs    # 消息处理和业务逻辑
├── renderer.rs      # 渲染状态和屏幕管理
├── wayland.rs       # Wayland compositor 核心状态（App 结构体）
├── backend.rs       # 渲染后端 trait 抽象
├── winit.rs         # winit 窗口后端实现
├── drm.rs           # DRM 物理屏幕后端实现
└── handlers/        # Wayland 协议处理器
    ├── mod.rs       # Seat、DataDevice 等处理器
    ├── compositor.rs # CompositorHandler、BufferHandler
    └── xdg_shell.rs # XdgShellHandler
```

## 开发指南

### 编译命令

```bash
# 运行（winit 后端）
cargo run -- --backend winit <command> [args...]

# 运行（DRM 后端）
sudo cargo run -- --backend drm <command> [args...]

# 检查代码
cargo check

# 格式化
cargo fmt

# 代码检查
cargo clippy
```

## 屏幕操作 Socket 协议

应用通过环境变量`MYDE_WRAP_SOCKET`获取 Socket 路径，格式为`[4字节大端长度][JSON消息体]`。

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

#### 截取矩形区域并渲染

```json
{
    "type": "RenderToScreen",
    "screen_index": 0,
    "rects": [{ "x": 0, "y": 0, "width": 800, "height": 600 }],
    "transforms": [{ "rotation": 15.0 }]
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

TODO:倾斜似乎没有保持直角

## 测试

### 手动测试

```bash
# 使用 winit 后端启动合成器
cargo run -- --backend winit /usr/bin/weston-terminal
```

实际物理drm输出需要在虚拟机上测试，故不对agent做要求
