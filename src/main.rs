mod protocol;
mod socket;
mod renderer;
mod compositor;

use std::sync::Arc;
use std::process::Command;
use tokio::sync::Mutex;
use tracing::{info, error};
use tracing_subscriber::EnvFilter;
use clap::Parser;

use crate::socket::SocketServer;
use crate::renderer::Renderer;
use crate::compositor::Compositor;

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

    std::env::set_var("MYDE_WRAP_SOCKET", socket_path.to_str().unwrap());
    info!("MYDE_WRAP_SOCKET={:?}", socket_path);

    let program = &args.command[0];
    let program_args = if args.command.len() > 1 {
        &args.command[1..]
    } else {
        &[]
    };

    info!("启动程序: {} {:?}", program, program_args);
    let mut child = Command::new(program)
        .args(program_args)
        .env("MYDE_WRAP_SOCKET", socket_path.to_str().unwrap())
        .spawn()?;

    info!("程序已启动, PID: {}", child.id());

    let renderer = Arc::new(Mutex::new(Renderer::new()));
    let compositor = Arc::new(Compositor::new(renderer.clone()));

    info!("启动 myde-wrap 合成器...");

    tokio::spawn(async move {
        let status = child.wait();
        match status {
            Ok(status) => info!("程序退出, 状态: {}", status),
            Err(e) => error!("等待程序失败: {}", e),
        }
    });

    loop {
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
