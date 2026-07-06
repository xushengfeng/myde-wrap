mod backend;
mod compositor;
mod custom_element;
mod drm;
mod handlers;
mod protocol;
mod renderer;
mod socket;
mod wayland;
mod winit;

use clap::Parser;
use smithay::reexports::{
    calloop::{
        timer::{TimeoutAction, Timer},
        EventLoop,
    },
    wayland_server::Display,
};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::backend::RenderBackend;
use crate::compositor::Compositor;
use crate::drm::DrmBackend;
use crate::renderer::Renderer as MyRenderer;
use crate::socket::SocketServer;
use crate::wayland::App;
use crate::winit::WinitBackend;

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

    let is_drm = args.backend.as_str() == "drm";

    // Create and initialize the selected backend
    let mut backend: Box<dyn RenderBackend> = match args.backend.as_str() {
        "winit" => {
            info!("using winit backend");
            Box::new(WinitBackend::new())
        }
        _ => {
            info!("using DRM backend");
            Box::new(DrmBackend::new())
        }
    };

    backend.init(&mut event_loop, &mut state)?;
    info!("backend inited: {}", backend.name());

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

    info!("run program: {} {:?}", program, program_args);

    // 继承所有父进程环境变量
    let mut envs: Vec<(String, String)> = std::env::vars().collect();
    // 确保 socket 环境变量存在
    if !envs.iter().any(|(k, _)| k == "MYDE_WRAP_SOCKET") {
        envs.push((
            "MYDE_WRAP_SOCKET".to_string(),
            socket_path.to_str().unwrap().to_string(),
        ));
    }
    // 确保 Wayland 环境变量存在
    if !envs.iter().any(|(k, _)| k == "WAYLAND_DISPLAY") {
        envs.push((
            "WAYLAND_DISPLAY".to_string(),
            state.socket_name.to_str().unwrap().to_string(),
        ));
    }
    // 确保 XDG_SESSION_TYPE 环境变量存在
    if !envs.iter().any(|(k, _)| k == "XDG_SESSION_TYPE") {
        envs.push(("XDG_SESSION_TYPE".to_string(), "wayland".to_string()));
    }

    let mut child = Command::new(program)
        .args(program_args)
        .envs(envs)
        .spawn()?;

    info!("program started, PID: {}", child.id());

    // Spawn a task to handle child process exit
    std::thread::spawn(move || {
        let status = child.wait();
        match status {
            Ok(status) => info!("program exited, status: {}", status),
            Err(e) => error!("failed to wait for program: {}", e),
        }
    });

    // 将Socket服务器添加到事件循环中
    let loop_handle = event_loop.handle();
    let socket_server = Arc::new(socket_server);
    let renderer = Arc::new(Mutex::new(MyRenderer::new()));
    let compositor = Arc::new(Compositor::new(renderer.clone()));
    let backend = Arc::new(Mutex::new(backend));

    // 克隆socket_server以在事件循环中使用
    let socket_server_clone = socket_server.clone();

    // 在单独的线程中接受和处理socket连接，完全不阻塞主事件循环
    std::thread::spawn(move || {
        let rt = Arc::new(tokio::runtime::Runtime::new().unwrap());
        loop {
            if let Some(mut stream) = socket_server_clone.accept() {
                let compositor = compositor.clone();
                let rt_clone = rt.clone();
                // 为每个连接单独分配一个线程，长连接完全不干扰主循环
                std::thread::spawn(move || {
                    info!("received new socket connection");
                    // 这里不用设置太短的timeout，因为它在独立线程中，不会阻塞其他渲染逻辑
                    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(5000)));
                    loop {
                        match socket::read_message(&mut stream) {
                            Ok(msg) => {
                                info!("received message: {:?}", msg);
                                let response = rt_clone.block_on(compositor.handle_message(msg));
                                if let Err(e) = socket::write_message(&mut stream, &response) {
                                    error!("failed to send response: {}", e);
                                    break;
                                }
                            }
                            Err(e) => {
                                // Socket可能正常断开或超时
                                tracing::debug!("Socket connection closed or timed out: {}", e);
                                break;
                            }
                        }
                    }
                });
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    });

    // For DRM backend, add a timer to periodically render the space
    let backend_for_render = backend.clone();
    if is_drm {
        // Create a timer that fires immediately and then every 16ms (~60fps)
        let timer = Timer::immediate();
        let renderer_for_timer = renderer.clone();
        loop_handle.insert_source(timer, move |_event, _, state| {
            let mut backend_guard = backend_for_render.blocking_lock();
            backend_guard.dispatch();
            let configs = renderer_for_timer
                .blocking_lock()
                .get_screen_configs()
                .to_vec();
            backend_guard.render_space(state, &configs);
            TimeoutAction::ToDuration(Duration::from_millis(16)) // ~60fps
        })?;
    }

    // 设置默认全屏显示
    // 如果没有自定义配置，默认全屏显示应用到所有屏幕
    let renderer_clone = renderer.clone();
    let backend_for_default = backend.clone();

    // 在单独的线程中设置默认全屏显示
    std::thread::spawn(move || {
        // 等待一小段时间让应用启动
        std::thread::sleep(std::time::Duration::from_millis(100));

        // 获取渲染器配置

        let backend_guard = backend_for_default.blocking_lock();
        let backend_output_count = backend_guard.get_output_count();

        // 为每个屏幕设置默认全屏配置
        let mut renderer_guard = renderer_clone.blocking_lock();
        for screen_index in 0..backend_output_count {
            let config = renderer_guard.get_default_fullscreen_config(screen_index);
            info!(
                "setting default fullscreen config: screen {}, rects {:?}, transforms {:?}",
                screen_index, config.rects, config.transforms
            );
            // SAVE IT into the renderer so that DRM backend can actually use it
            let _ = renderer_guard.render_to_screen(
                screen_index,
                config.rects.clone(),
                config.transforms.clone(),
            );
        }
    });

    // Run the event loop
    info!("Starting event loop...");
    event_loop.run(None, &mut state, move |_| {
        // Wayland events are handled by the display source in init_wayland_listener
        // DRM rendering is handled by the timer above
    })?;

    Ok(())
}
