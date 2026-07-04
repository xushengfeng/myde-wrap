mod protocol;
mod socket;
mod renderer;
mod compositor;
mod wayland;

use std::sync::Arc;
use std::process::Command;
use tokio::sync::Mutex;
use tracing::{info, error};
use tracing_subscriber::EnvFilter;
use clap::Parser;

use crate::socket::SocketServer;
use crate::renderer::Renderer;
use crate::compositor::Compositor;
use crate::wayland::WaylandCompositor;
use crate::protocol::Transform;

#[derive(Parser)]
#[command(name = "myde-wrap")]
#[command(about = "轻量级 Wayland 桌面合成器")]
struct Args {
    /// 要渲染的程序命令
    #[arg(trailing_var_arg = true)]
    command: Vec<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    if args.command.is_empty() {
        eprintln!("错误: 请指定要渲染的程序");
        eprintln!("用法: myde-wrap <command> [args...]");
        std::process::exit(1);
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let socket_path = std::env::temp_dir().join("myde-wrap.sock");
    let socket_server = SocketServer::new(socket_path.clone())?;

    // 创建 Wayland compositor
    let mut wayland_compositor = WaylandCompositor::new();
    wayland_compositor.init_winit()?;

    // 获取 Wayland socket 名称（由 wayland compositor 设置）
    let wayland_display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());
    std::env::set_var("XDG_SESSION_TYPE", "wayland");
    std::env::set_var("MYDE_WRAP_SOCKET", socket_path.to_str().unwrap());

    info!("MYDE_WRAP_SOCKET={:?}", socket_path);
    info!("WAYLAND_DISPLAY={:?}", wayland_display);
    info!("XDG_SESSION_TYPE=wayland");

    let program = &args.command[0];
    let program_args = if args.command.len() > 1 {
        &args.command[1..]
    } else {
        &[]
    };

    info!("启动程序: {} {:?}", program, program_args);

    // 继承所有父进程环境变量
    let mut envs: Vec<(String, String)> = std::env::vars().collect();
    // 确保 socket 环境变量存在
    if !envs.iter().any(|(k, _)| k == "MYDE_WRAP_SOCKET") {
        envs.push(("MYDE_WRAP_SOCKET".to_string(), socket_path.to_str().unwrap().to_string()));
    }
    // 确保 Wayland 环境变量存在
    if !envs.iter().any(|(k, _)| k == "WAYLAND_DISPLAY") {
        envs.push(("WAYLAND_DISPLAY".to_string(), wayland_display.to_string()));
    }
    // 确保 XDG_SESSION_TYPE 环境变量存在
    if !envs.iter().any(|(k, _)| k == "XDG_SESSION_TYPE") {
        envs.push(("XDG_SESSION_TYPE".to_string(), "wayland".to_string()));
    }

    let mut child = Command::new(program)
        .args(program_args)
        .envs(envs)
        .spawn()?;

    info!("程序已启动, PID: {}", child.id());

    let renderer = Arc::new(Mutex::new(Renderer::new()));
    let compositor = Arc::new(Compositor::new(renderer.clone()));

    // 获取默认屏幕尺寸并设置到 compositor
    let screens = {
        let r = renderer.lock().await;
        r.get_screens()
    };
    if let Some(screen) = screens.first() {
        wayland_compositor.set_size(screen.width, screen.height);
    }

    info!("启动 myde-wrap 合成器...");
    info!("输出数量: {}", wayland_compositor.get_output_count());

    tokio::spawn(async move {
        let status = child.wait();
        match status {
            Ok(status) => info!("程序退出, 状态: {}", status),
            Err(e) => error!("等待程序失败: {}", e),
        }
    });

    loop {
        // 处理 Wayland 事件
        wayland_compositor.dispatch();

        // 获取屏幕配置并渲染
        let screen_configs = {
            let r = renderer.lock().await;
            r.get_screen_configs().to_vec()
        };

        for config in &screen_configs {
            for (rect, transform) in config.rects.iter().zip(config.transforms.iter()) {
                wayland_compositor.render_rect_to_screen(
                    config.screen_index,
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    transform,
                );
            }
        }

        // 如果没有屏幕配置，使用默认渲染
        if screen_configs.is_empty() {
            let rects = {
                let r = renderer.lock().await;
                r.get_captured_rects().to_vec()
            };
            for rect in &rects {
                wayland_compositor.render_rect(rect.x, rect.y, rect.width, rect.height);
            }
        }

        // 处理 Socket 连接
        if let Some(mut stream) = socket_server.accept() {
            let compositor = compositor.clone();
            tokio::spawn(async move {
                loop {
                    match socket::read_message(&mut stream) {
                        Ok(msg) => {
                            info!("收到消息: {:?}", msg);
                            let response = compositor.handle_message(msg).await;
                            if let Err(e) = socket::write_message(&mut stream, &response) {
                                error!("写入错误: {}", e);
                                break;
                            }
                        }
                        Err(e) => {
                            error!("读取错误: {}", e);
                            break;
                        }
                    }
                }
            });
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }
}
