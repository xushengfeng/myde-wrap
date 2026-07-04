# AGENTS.md

## 项目概述

myde-wrap 是一个轻量级的 Rust Wayland 桌面合成器，专注于渲染单个应用，并通过 Unix Socket 提供远程控制能力。

## 技术架构

### 核心模块

```
src/
├── main.rs          # 程序入口，初始化日志、Socket、事件循环
├── protocol.rs      # 协议数据结构定义
├── socket.rs        # Unix Socket 服务端实现
├── compositor.rs    # 消息处理和业务逻辑
└── renderer.rs      # 渲染状态和屏幕管理
```

### 数据流

```
客户端 → Socket → read_message() → Compositor.handle_message() → Renderer → write_message() → 客户端
```

### 关键依赖

- **smithay** (v0.3): Wayland 合成器框架
- **tokio** (v1): 异步运行时，用于主事件循环
- **serde** / **serde_json**: JSON 序列化/反序列化
- **tracing**: 结构化日志

## 开发指南

### 编译命令

```bash
# 开发编译
cargo build

# 发布编译
cargo build --release

# 运行
cargo run -- <command> [args...]

# 检查代码
cargo check

# 格式化
cargo fmt

# 代码检查
cargo clippy
```

### 添加新功能

1. 在 `protocol.rs` 中定义新的消息类型
2. 在 `renderer.rs` 中实现渲染逻辑
3. 在 `compositor.rs` 中添加消息处理
4. 确保编译通过: `cargo build`

### Socket 协议规范

- **消息格式**: `[4字节大端长度][JSON消息体]`
- **最大消息大小**: 10MB
- **编码**: UTF-8 JSON
- **字节序**: 大端序（网络字节序）

### 环境变量

- `MYDE_WRAP_SOCKET`: Socket 文件路径（程序自动设置）
- `RUST_LOG`: 日志级别（如 `debug`, `info`, `warn`, `error`）

## 代码规范

### 命名约定

- 结构体: PascalCase (`ClientMessage`, `ServerResponse`)
- 函数/方法: snake_case (`handle_message`, `read_message`)
- 常量: SCREAMING_SNAKE_CASE
- 文件名: snake_case (`protocol.rs`, `socket.rs`)

### 错误处理

- 使用 `anyhow::Result` 进行错误传播
- Socket 错误应断开连接
- 业务错误返回 `ServerResponse::Error`

### 并发模型

- 主循环在 `tokio::main` 中运行
- 每个客户端连接在独立的 tokio task 中处理
- Renderer 使用 `Arc<Mutex<Renderer>>` 共享状态
- 子进程在独立的 tokio task 中管理，监控其退出状态

## 数据结构

### 核心类型

```rust
// 矩形区域
pub struct Rect {
    pub x: i32,      // 左上角 x 坐标（相对于窗口左上角）
    pub y: i32,      // 左上角 y 坐标（相对于窗口左上角）
    pub width: u32,  // 矩形宽度
    pub height: u32, // 矩形高度
}

// 变换参数
// 变换顺序：先缩放，后平移
// 缩放原点：矩形左上角 (rect.x, rect.y)
pub struct Transform {
    pub scale_x: f64,      // X 轴缩放因子（1.0 = 不缩放）
    pub scale_y: f64,      // Y 轴缩放因子（1.0 = 不缩放）
    pub translate_x: f64,  // X 轴平移量（像素，正值向右）
    pub translate_y: f64,  // Y 轴平移量（像素，正值向下）
}

// 屏幕信息
pub struct ScreenInfo {
    pub name: String,         // 屏幕名称（如 "Screen-0"、"eDP-1"）
    pub width: u32,           // 屏幕宽度（像素）
    pub height: u32,          // 屏幕高度（像素）
    pub refresh_rate: u32,    // 刷新率（Hz）
}
```

### 变换公式

```
final_x = rect.x * scale_x + translate_x
final_y = rect.y * scale_y + translate_y
final_width = rect.width * scale_x
final_height = rect.height * scale_y
```

## 测试

### 手动测试

```bash
# 启动合成器
cargo run

# 在另一个终端中运行测试客户端
# 确保 MYDE_WRAP_SOCKET 环境变量已设置
```

### 单元测试

```bash
cargo test
```

## 调试

### 启用详细日志

```bash
RUST_LOG=debug cargo run
```

### 查看 Socket 通信

```bash
# 使用 socat 监控 Socket
socat - UNIX-CONNECT:$MYDE_WRAP_SOCKET
```

## 扩展点

### 添加新的屏幕后端

在 `renderer.rs` 中的 `screens` 字段可以扩展为动态获取实际屏幕信息。

### 添加新的渲染目标

`RenderToScreen` 消息可以扩展为支持更多渲染目标。

### 添加新的变换类型

`Transform` 结构体可以扩展支持旋转、裁剪等更多变换操作。
