mod protocol;
mod socket;
mod renderer;
mod compositor;
mod wayland;
mod backend;
mod winit;
mod drm;
mod handlers;

use std::process::Command;
use tracing::{info, error};
use clap::Parser;
use smithay::reexports::{calloop::EventLoop, wayland_server::Display};

use crate::socket::SocketServer;
use crate::wayland::App;
use crate::backend::RenderBackend;
use crate::winit::WinitBackend;
use crate::drm::DrmBackend;

#[derive(Parser)]
#[command(name = "myde-wrap")]
#[command(about = "轻量级 Wayland 桌面合成器")]
struct Args {
    /// 要渲染的程序命令
    #[arg(trailing_var_arg = true)]
    command: Vec<String>,

    /// 渲染后端 (winit 或 drm)
    #[arg(short, long, default_value = "winit")]
    backend: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if args.command.is_empty() {
        eprintln!("错误: 请指定要渲染的程序");
        eprintln!("用法: myde-wrap [--backend winit|drm] <command> [args...]");
        std::process::exit(1);
    }

    // Initialize logging
    if let Ok(env_filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    } else {
        tracing_subscriber::fmt().init();
    }

    let socket_path = std::env::temp_dir().join("myde-wrap.sock");
    let socket_server = SocketServer::new(socket_path.clone())?;

    let mut event_loop: EventLoop<App> = EventLoop::try_new()?;
    let display: Display<App> = Display::new()?;
    let mut state = App::new(&mut event_loop, display);

    // Create and initialize the selected backend
    let mut backend: Box<dyn RenderBackend> = match args.backend.as_str() {
        "drm" => {
            info!("使用 DRM 后端");
            Box::new(DrmBackend::new())
        }
        _ => {
            info!("使用 winit 后端");
            Box::new(WinitBackend::new())
        }
    };

    backend.init(&mut event_loop, &mut state)?;
    info!("后端初始化完成: {}", backend.name());

    // Set WAYLAND_DISPLAY to our socket name, so child processes connect to myde-wrap
    // rather than the host compositor
    unsafe { std::env::set_var("WAYLAND_DISPLAY", &state.socket_name) };
    unsafe { std::env::set_var("XDG_SESSION_TYPE", "wayland") };
    unsafe { std::env::set_var("MYDE_WRAP_SOCKET", socket_path.to_str().unwrap()) };

    info!("MYDE_WRAP_SOCKET={:?}", socket_path);
    info!("WAYLAND_DISPLAY={:?}", state.socket_name);
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
        envs.push(("WAYLAND_DISPLAY".to_string(), state.socket_name.to_str().unwrap().to_string()));
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

    // Spawn a task to handle child process exit
    std::thread::spawn(move || {
        let status = child.wait();
        match status {
            Ok(status) => info!("程序退出, 状态: {}", status),
            Err(e) => error!("等待程序失败: {}", e),
        }
    });

    // Run the event loop
    event_loop.run(None, &mut state, move |_| {
        // myde-wrap is running
    })?;

    Ok(())
}
